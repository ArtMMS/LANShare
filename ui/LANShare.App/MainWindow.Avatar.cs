using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.Shapes;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.Media;
using Avalonia.Media.Imaging;
using Avalonia.Platform.Storage;
using Avalonia.Threading;

namespace LANShare.App;

/// Cartão de perfil (avatar + nome + configurações) e o painel de troca de avatar.
public partial class MainWindow
{
    // Área de recorte: quadrado de 280 com o círculo de 240 no meio
    private const double CropView = 280;
    private const double CropCircle = 240;
    private const double CropInset = (CropView - CropCircle) / 2;

    private static readonly IBrush NoteBrush = new SolidColorBrush(Color.Parse("#9AA4B2"));
    private static readonly IBrush ErrorBrush = new SolidColorBrush(Color.Parse("#FF6B6B"));

    private Border _avatarButton = null!, _avatarEditor = null!, _avatarDropZone = null!, _avatarCropArea = null!;
    private Ellipse _avatarImage = null!;
    private Image _avatarCropImage = null!;
    private Slider _avatarZoomSlider = null!;
    private Grid _avatarZoomRow = null!;
    private TextBlock _avatarEditorError = null!;
    private Button _avatarChooseButton = null!, _avatarRemoveButton = null!, _avatarCancelButton = null!,
        _avatarSaveButton = null!, _settingsButton = null!;

    // Avatar mostrado no cartão (com animação, se for GIF)
    private readonly ImageBrush _avatarBrush = new() { Stretch = Stretch.UniformToFill };
    private readonly DispatcherTimer _avatarTimer = new();
    private AvatarImage? _avatar;
    private int _avatarFrame;

    private bool _avatarEditorOpen;

    // Imagem sendo recortada e a posição dela dentro da área de recorte
    private Bitmap? _cropSource;
    private byte[]? _cropGifBytes; // só quando a imagem escolhida é um GIF
    private double _cropMinScale = 1, _cropScale = 1, _cropX, _cropY;
    private Point? _cropDragFrom;

    /// Chamado uma vez no construtor.
    private void InitAvatar()
    {
        _avatarButton = Find<Border>("AvatarButton");
        _avatarImage = Find<Ellipse>("AvatarImage");
        _settingsButton = Find<Button>("SettingsButton");
        _avatarEditor = Find<Border>("AvatarEditor");
        _avatarDropZone = Find<Border>("AvatarDropZone");
        _avatarCropArea = Find<Border>("AvatarCropArea");
        _avatarCropImage = Find<Image>("AvatarCropImage");
        _avatarZoomSlider = Find<Slider>("AvatarZoomSlider");
        _avatarZoomRow = Find<Grid>("AvatarZoomRow");
        _avatarEditorError = Find<TextBlock>("AvatarEditorError");
        _avatarChooseButton = Find<Button>("AvatarChooseButton");
        _avatarRemoveButton = Find<Button>("AvatarRemoveButton");
        _avatarCancelButton = Find<Button>("AvatarCancelButton");
        _avatarSaveButton = Find<Button>("AvatarSaveButton");

        _avatarButton.PointerPressed += (_, e) =>
        {
            if (e.GetCurrentPoint(_avatarButton).Properties.IsLeftButtonPressed) OpenAvatarEditor();
        };
        _settingsButton.Click += (_, _) => { }; // a janela de configurações vem depois

        _avatarDropZone.PointerPressed += async (_, _) => await PickAvatarFileAsync();
        _avatarChooseButton.Click += async (_, _) => await PickAvatarFileAsync();
        _avatarCancelButton.Click += (_, _) => CloseAvatarEditor();
        _avatarSaveButton.Click += async (_, _) => await SaveAvatarAsync();
        _avatarRemoveButton.Click += (_, _) => RemoveAvatar();
        _avatarZoomSlider.ValueChanged += (_, _) => OnCropZoomChanged();

        // Arrastar a imagem dentro do círculo
        _avatarCropArea.PointerPressed += (_, e) =>
        {
            _cropDragFrom = e.GetPosition(_avatarCropArea);
            e.Pointer.Capture(_avatarCropArea);
            e.Handled = true;
        };
        _avatarCropArea.PointerMoved += (_, e) =>
        {
            if (_cropDragFrom is not { } from) return;
            var now = e.GetPosition(_avatarCropArea);
            _cropX += now.X - from.X;
            _cropY += now.Y - from.Y;
            _cropDragFrom = now;
            ApplyCrop();
        };
        _avatarCropArea.PointerReleased += (_, e) =>
        {
            _cropDragFrom = null;
            e.Pointer.Capture(null);
        };
        _avatarCropArea.PointerCaptureLost += (_, _) => _cropDragFrom = null;

        // Roda do mouse = zoom
        _avatarCropArea.PointerWheelChanged += (_, e) =>
        {
            _avatarZoomSlider.Value += e.Delta.Y * 0.15;
            e.Handled = true;
        };

        // Soltar uma imagem em qualquer lugar do painel
        DragDrop.SetAllowDrop(_avatarEditor, true);
        _avatarEditor.AddHandler(DragDrop.DragOverEvent, OnAvatarDragOver);
        _avatarEditor.AddHandler(DragDrop.DropEvent, OnAvatarDrop);

        // Clicar na parte escurecida (fora do painel) cancela
        _avatarEditor.PointerPressed += (_, e) =>
        {
            if (ReferenceEquals(e.Source, _avatarEditor)) CloseAvatarEditor();
        };

        AddHandler(KeyDownEvent, OnAvatarKeyDown, RoutingStrategies.Tunnel);

        // Animação do GIF: cada quadro tem o seu próprio tempo
        _avatarTimer.Tick += (_, _) => NextAvatarFrame();

        _ = LoadAvatarAsync();
    }

    // ───────────────────────── Avatar no cartão ─────────────────────────

    /// Carrega o avatar salvo sem travar a janela (um GIF leva um instante para preparar).
    private async Task LoadAvatarAsync()
    {
        AvatarImage? avatar;
        try
        {
            avatar = await Task.Run(AvatarStore.Load);
        }
        catch (Exception)
        {
            avatar = AvatarStore.LoadDefault();
        }
        ApplyAvatar(avatar);
    }

    private void ApplyAvatar(AvatarImage? avatar)
    {
        _avatarTimer.Stop();
        _avatar = avatar;
        _avatarFrame = 0;

        if (avatar is null)
        {
            _avatarImage.Fill = new SolidColorBrush(Color.Parse("#2E343C"));
            return;
        }

        _avatarBrush.Source = avatar.Frames[0];
        _avatarImage.Fill = _avatarBrush;
        _avatarImage.InvalidateVisual();

        if (avatar.IsAnimated)
        {
            _avatarTimer.Interval = avatar.Delays[0];
            _avatarTimer.Start();
        }
    }

    private void NextAvatarFrame()
    {
        if (_avatar is not { IsAnimated: true } avatar)
        {
            _avatarTimer.Stop();
            return;
        }

        _avatarFrame = (_avatarFrame + 1) % avatar.Frames.Count;
        _avatarBrush.Source = avatar.Frames[_avatarFrame];
        _avatarTimer.Interval = avatar.Delays[_avatarFrame];
        _avatarImage.InvalidateVisual();
    }

    private void OnAvatarKeyDown(object? sender, KeyEventArgs e)
    {
        if (!_avatarEditorOpen || e.Key != Key.Escape) return;
        CloseAvatarEditor();
        e.Handled = true;
    }

    // ───────────────────────── Painel do avatar ─────────────────────────

    private void OpenAvatarEditor()
    {
        if (_avatarEditorOpen || _pickerOpen) return;

        _avatarEditorOpen = true;
        ClearCrop();
        SetAvatarMessage(null);
        _avatarRemoveButton.IsVisible = AvatarStore.HasCustom;

        _avatarEditor.IsVisible = true;
        _mainLayout.IsEnabled = false;
    }

    private void CloseAvatarEditor()
    {
        if (!_avatarEditorOpen) return;

        _avatarEditorOpen = false;
        _avatarEditor.IsVisible = false;
        _mainLayout.IsEnabled = true;
        ClearCrop();
    }

    /// Volta o painel para "escolha ou arraste uma imagem".
    private void ClearCrop()
    {
        _avatarCropImage.Source = null;
        _cropSource?.Dispose();
        _cropSource = null;
        _cropGifBytes = null;
        _cropDragFrom = null;

        _avatarDropZone.IsVisible = true;
        _avatarCropArea.IsVisible = false;
        _avatarZoomRow.IsVisible = false;
        _avatarChooseButton.IsVisible = false;
        _avatarSaveButton.IsEnabled = false;
    }

    /// Mensagem embaixo da imagem: aviso (cinza) ou erro (vermelho).
    private void SetAvatarMessage(string? message, bool isError = false)
    {
        _avatarEditorError.Text = message ?? "";
        _avatarEditorError.Foreground = isError ? ErrorBrush : NoteBrush;
        _avatarEditorError.IsVisible = !string.IsNullOrEmpty(message);
    }

    private async Task PickAvatarFileAsync()
    {
        var files = await StorageProvider.OpenFilePickerAsync(new FilePickerOpenOptions
        {
            Title = "Escolha uma imagem para o avatar",
            AllowMultiple = false,
            FileTypeFilter = new[] { FilePickerFileTypes.ImageAll },
        });

        if (files.Count > 0) await LoadCropFileAsync(files[0]);
    }

    private void OnAvatarDragOver(object? sender, DragEventArgs e)
    {
        e.DragEffects = e.DataTransfer.Contains(DataFormat.File) ? DragDropEffects.Copy : DragDropEffects.None;
        e.Handled = true;
    }

    private async void OnAvatarDrop(object? sender, DragEventArgs e)
    {
        e.Handled = true;
        var file = e.DataTransfer.TryGetFiles()?.OfType<IStorageFile>().FirstOrDefault();
        if (file is not null) await LoadCropFileAsync(file);
    }

    private async Task LoadCropFileAsync(IStorageFile file)
    {
        try
        {
            byte[] bytes;
            await using (var stream = await file.OpenReadAsync())
            using (var memory = new MemoryStream())
            {
                await stream.CopyToAsync(memory);
                bytes = memory.ToArray();
            }

            var isGif = AvatarStore.IsGif(bytes);
            if (isGif && bytes.Length > AvatarStore.MaxGifBytes)
            {
                SetAvatarMessage("Esse GIF é grande demais (máximo de 15 MB).", isError: true);
                return;
            }

            // Para um GIF, o recorte mostra o primeiro quadro
            var bitmap = new Bitmap(new MemoryStream(bytes));
            if (bitmap.PixelSize.Width < 2 || bitmap.PixelSize.Height < 2)
            {
                bitmap.Dispose();
                SetAvatarMessage("Essa imagem é pequena demais.", isError: true);
                return;
            }

            StartCrop(bitmap, isGif ? bytes : null);
            SetAvatarMessage(isGif ? "GIF: a animação aparece no seu avatar depois de salvar." : null);
        }
        catch (Exception)
        {
            SetAvatarMessage("Não foi possível abrir essa imagem. Use PNG, JPG, GIF, BMP ou WEBP.", isError: true);
        }
    }

    // ───────────────────────── Recorte ─────────────────────────

    /// Mostra a imagem na área de recorte, cobrindo o círculo e centralizada.
    private void StartCrop(Bitmap bitmap, byte[]? gifBytes)
    {
        _cropSource?.Dispose();
        _cropSource = bitmap;
        _cropGifBytes = gifBytes;
        _avatarCropImage.Source = bitmap;

        var (w, h) = (bitmap.PixelSize.Width, bitmap.PixelSize.Height);
        _cropMinScale = Math.Max(CropCircle / w, CropCircle / h);
        _cropScale = _cropMinScale;
        _cropX = (CropView - w * _cropScale) / 2;
        _cropY = (CropView - h * _cropScale) / 2;

        _avatarZoomSlider.Value = 1;
        ApplyCrop();

        _avatarDropZone.IsVisible = false;
        _avatarCropArea.IsVisible = true;
        _avatarZoomRow.IsVisible = true;
        _avatarChooseButton.IsVisible = true;
        _avatarSaveButton.IsEnabled = true;
    }

    /// Zoom em volta do centro do círculo.
    private void OnCropZoomChanged()
    {
        if (_cropSource is null) return;

        const double center = CropView / 2;
        var imageX = (center - _cropX) / _cropScale;
        var imageY = (center - _cropY) / _cropScale;

        _cropScale = _cropMinScale * _avatarZoomSlider.Value;
        _cropX = center - imageX * _cropScale;
        _cropY = center - imageY * _cropScale;
        ApplyCrop();
    }

    /// Posiciona a imagem, sem deixar o círculo ficar com partes vazias.
    private void ApplyCrop()
    {
        if (_cropSource is null) return;

        var w = _cropSource.PixelSize.Width * _cropScale;
        var h = _cropSource.PixelSize.Height * _cropScale;

        _cropX = Math.Clamp(_cropX, CropInset + CropCircle - w, CropInset);
        _cropY = Math.Clamp(_cropY, CropInset + CropCircle - h, CropInset);

        _avatarCropImage.Width = w;
        _avatarCropImage.Height = h;
        Canvas.SetLeft(_avatarCropImage, _cropX);
        Canvas.SetTop(_avatarCropImage, _cropY);
    }

    private async Task SaveAvatarAsync()
    {
        if (_cropSource is null) return;

        // O pedaço da imagem (em pixels) que está dentro do círculo
        var crop = new Rect(
            (CropInset - _cropX) / _cropScale,
            (CropInset - _cropY) / _cropScale,
            CropCircle / _cropScale,
            CropCircle / _cropScale);

        _avatarSaveButton.IsEnabled = false;
        try
        {
            if (_cropGifBytes is { } gif)
            {
                await Task.Run(() => AvatarStore.SaveGif(gif, crop));
            }
            else
            {
                AvatarStore.SaveStill(_cropSource, crop);
            }
        }
        catch (Exception ex)
        {
            SetAvatarMessage("Não foi possível salvar o avatar: " + ex.Message, isError: true);
            _avatarSaveButton.IsEnabled = true;
            return;
        }

        CloseAvatarEditor();
        await LoadAvatarAsync();
    }

    private void RemoveAvatar()
    {
        try
        {
            AvatarStore.Remove();
        }
        catch (Exception ex)
        {
            SetAvatarMessage("Não foi possível remover o avatar: " + ex.Message, isError: true);
            return;
        }

        ApplyAvatar(AvatarStore.LoadDefault());
        CloseAvatarEditor();
    }
}