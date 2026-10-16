using Avalonia;
using Avalonia.Animation;
using Avalonia.Controls;
using Avalonia.Layout;
using Avalonia.Media;
using Avalonia.Threading;

namespace LANShare.App;

/// Avisos rápidos no canto da janela, com som: alguém entrou/saiu, transmissão começou.
public partial class MainWindow
{
    private static readonly TimeSpan ToastDuration = TimeSpan.FromSeconds(4);
    private static readonly TimeSpan ToastFade = TimeSpan.FromMilliseconds(200);
    private const int MaxToasts = 4;

    private const string JoinGlyph = "\uE8FA";
    private const string LeaveGlyph = "\uE72B";
    private const string KickGlyph = "\uE7BA";
    private const string StreamGlyph = "\uE7F4";

    private StackPanel _toastHost = null!;

    /// Chamado uma vez no construtor.
    private void InitNotifications()
    {
        _toastHost = new StackPanel
        {
            Spacing = 8,
            Width = 300,
            HorizontalAlignment = HorizontalAlignment.Right,
            VerticalAlignment = VerticalAlignment.Top,
            Margin = new Thickness(16),
            IsHitTestVisible = false, // não atrapalha cliques na janela
            ZIndex = 100,             // fica por cima até das configurações
        };

        // Por cima de tudo, no Grid raiz da janela
        if (Content is Panel root) root.Children.Add(_toastHost);
    }

    /// Mostra um aviso e toca o som (se ele estiver ligado nas configurações).
    private void Notify(string message, string glyph, Sound? sound = null)
    {
        if (sound is { } s && SoundAllowed(s)) Sounds.Play(s);

        // Muitos avisos de uma vez: some o mais antigo
        while (_toastHost.Children.Count >= MaxToasts) _toastHost.Children.RemoveAt(0);

        var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 10 };
        row.Children.Add(GlyphIcon(glyph, 16));
        row.Children.Add(new TextBlock
        {
            Text = message,
            TextWrapping = TextWrapping.Wrap,
            MaxWidth = 240,
            VerticalAlignment = VerticalAlignment.Center,
        });

        var toast = new Border
        {
            Background = new SolidColorBrush(Color.Parse("#1B1F24")),
            BorderBrush = new SolidColorBrush(Color.Parse("#2E343C")),
            BorderThickness = new Thickness(1),
            CornerRadius = new CornerRadius(10),
            Padding = new Thickness(14, 10),
            BoxShadow = BoxShadows.Parse("0 6 20 0 #80000000"),
            Opacity = 0,
            Child = row,
            Transitions = new Transitions
            {
                new DoubleTransition { Property = OpacityProperty, Duration = ToastFade },
            },
        };

        _toastHost.Children.Add(toast);
        Dispatcher.UIThread.Post(() => toast.Opacity = 1, DispatcherPriority.Background);

        DispatcherTimer.RunOnce(() =>
        {
            toast.Opacity = 0;
            DispatcherTimer.RunOnce(() => _toastHost.Children.Remove(toast), ToastFade);
        }, ToastDuration);
    }
}