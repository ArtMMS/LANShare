using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.Shapes;
using Avalonia.Layout;
using Avalonia.Media;
using Avalonia.Media.Imaging;

namespace LANShare.App;

/// Tela da sala (v2.0), no Host e no Client: todos da sala em grade, como no Discord / Google Meet.
/// - a grade se ajusta ao número de pessoas (1, 2, 3–4, 5–6, 7–9...): colunas = ⌈√n⌉, a última
///   fileira fica centralizada;
/// - cada quadrado mostra o avatar (a câmera entra aqui depois), o nome e o ícone de tela de quem
///   transmite (clicar assiste, como na lista);
/// - o quadrado de quem toca um efeito sonoro acende (borda azul + 🎉), como na lista;
/// - aparece na área grande quando estou na sala e ela está sem imagem (não estou assistindo
///   ninguém e a prévia da minha transmissão não está na tela).
public partial class MainWindow
{
    private const double RoomTileGap = 12;
    private const double RoomTileAspect = 16.0 / 9.0;

    /// Com pouca gente, os quadrados não passam deste tamanho (1 pessoa não ocupa a tela toda).
    private const double RoomTileMaxWidth = 640;
    private const double RoomTileMinWidth = 120;

    private static readonly IBrush RoomTileBackground = new SolidColorBrush(Color.Parse("#1B1F24"));
    private static readonly IBrush RoomTileBorderIdle = new SolidColorBrush(Color.Parse("#262C34"));
    private static readonly IBrush RoomNameBackground = new SolidColorBrush(Color.Parse("#B3000000"));

    /// Uma pessoa da sala, do jeito que aparece na grade.
    private sealed record RoomMember(ulong Id, string Name, Bitmap? Avatar);

    private Panel _roomView = null!;
    private StackPanel _roomRows = null!;
    private List<RoomMember> _roomMembers = new();

    /// Chamado uma vez no construtor (antes de qualquer UpdateStage).
    private void InitRoomView()
    {
        _roomView = Find<Panel>("RoomView");
        _roomRows = Find<StackPanel>("RoomRows");

        // O tamanho dos quadrados depende do espaço: refaz ao redimensionar (e ao aparecer)
        _roomView.SizeChanged += (_, _) => LayoutRoomTiles();
    }

    /// Estou numa sala? (Host com a sala aberta ou Client conectado.)
    private bool InRoom =>
        (_mode == Mode.Host && _roomOpen) || (_mode == Mode.Client && _clientState == ClientState.Connected);

    /// A grade aparece quando estou na sala e a área grande está sem imagem.
    private bool RoomViewVisible =>
        InRoom && _watchState == WatchState.None && !_previewImage.IsVisible;

    /// Troca quem aparece na grade (chamado junto com a lista da lateral).
    private void SetRoomMembers(List<RoomMember> members)
    {
        _roomMembers = members;
        LayoutRoomTiles();
    }

    /// Colunas e fileiras para `count` pessoas: 1 → 1x1, 2 → 2x1, 3–4 → 2x2, 5–6 → 3x2, 7–9 → 3x3...
    private static (int Columns, int Rows) RoomGridSize(int count)
    {
        if (count <= 0) return (0, 0);
        var columns = (int)Math.Ceiling(Math.Sqrt(count));
        var rows = (int)Math.Ceiling(count / (double)columns);
        return (columns, rows);
    }

    /// Monta a grade de novo, com o maior quadrado 16:9 que cabe no espaço.
    private void LayoutRoomTiles()
    {
        _roomRows.Children.Clear();

        var count = _roomMembers.Count;
        if (count == 0) return;

        // Ainda sem tamanho (escondida): o SizeChanged chama de novo quando ela aparecer
        var area = _roomView.Bounds.Size;
        if (area.Width <= 0 || area.Height <= 0) return;

        var (columns, rows) = RoomGridSize(count);
        var byWidth = (area.Width - RoomTileGap * (columns - 1)) / columns;
        var byHeight = (area.Height - RoomTileGap * (rows - 1)) / rows * RoomTileAspect;
        var width = Math.Clamp(Math.Min(byWidth, byHeight), RoomTileMinWidth, RoomTileMaxWidth);
        var height = width / RoomTileAspect;

        _roomRows.Spacing = RoomTileGap;
        for (var r = 0; r < rows; r++)
        {
            var row = new StackPanel
            {
                Orientation = Orientation.Horizontal,
                Spacing = RoomTileGap,
                HorizontalAlignment = HorizontalAlignment.Center,
            };
            foreach (var member in _roomMembers.Skip(r * columns).Take(columns))
                row.Children.Add(BuildRoomTile(member, width, height));
            _roomRows.Children.Add(row);
        }
    }

    /// Um quadrado da grade: avatar no meio, nome (+ ícone de tela) embaixo à esquerda
    /// e 🎉 em cima à direita enquanto a pessoa toca um efeito sonoro.
    private Control BuildRoomTile(RoomMember member, double width, double height)
    {
        var lit = _sfxHighlights.TryGetValue(member.Id, out var until) && until > DateTime.UtcNow;
        var content = new Grid();

        // Avatar (quando a câmera existir, a imagem dela entra no lugar)
        var avatarSize = Math.Clamp(height * 0.42, 40, 160);
        content.Children.Add(new Ellipse
        {
            Width = avatarSize,
            Height = avatarSize,
            Fill = AvatarBrush(member.Avatar),
            HorizontalAlignment = HorizontalAlignment.Center,
            VerticalAlignment = VerticalAlignment.Center,
        });

        // Nome + ícone de tela
        var label = new StackPanel
        {
            Orientation = Orientation.Horizontal,
            Spacing = 6,
            VerticalAlignment = VerticalAlignment.Center,
        };
        label.Children.Add(new TextBlock
        {
            Text = member.Name,
            FontSize = 13,
            MaxWidth = Math.Max(40, width - 80),
            VerticalAlignment = VerticalAlignment.Center,
            TextTrimming = TextTrimming.CharacterEllipsis,
        });
        if (IsStreamingUser(member.Id)) label.Children.Add(ScreenIcon(member.Id));

        content.Children.Add(new Border
        {
            Background = RoomNameBackground,
            CornerRadius = new CornerRadius(6),
            Padding = new Thickness(8, 3),
            Margin = new Thickness(10),
            HorizontalAlignment = HorizontalAlignment.Left,
            VerticalAlignment = VerticalAlignment.Bottom,
            Child = label,
        });

        if (lit)
        {
            content.Children.Add(new TextBlock
            {
                Text = PartyEmoji,
                FontFamily = SfxEmojiFont,
                FontSize = 18,
                Margin = new Thickness(10),
                HorizontalAlignment = HorizontalAlignment.Right,
                VerticalAlignment = VerticalAlignment.Top,
            });
        }

        return new Border
        {
            Width = width,
            Height = height,
            CornerRadius = new CornerRadius(12),
            Background = RoomTileBackground,
            BorderThickness = new Thickness(2),
            BorderBrush = lit ? SfxHighlightBorder : RoomTileBorderIdle,
            ClipToBounds = true,
            Child = content,
        };
    }
}