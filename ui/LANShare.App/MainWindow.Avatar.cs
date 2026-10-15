using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.Shapes;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.Media;
using Avalonia.Media.Imaging;
using Avalonia.Platform.Storage;

namespace LANShare.App;

/// Cartão de perfil (avatar + nome + configurações) e o painel de troca de avatar.
public partial class MainWindow
{
    // Área de recorte: quadrado de 280 com o círculo de 240 no meio
    private const double CropView = 280;
    private const double CropCircle = 240;
    private const double CropInset = (CropView - CropCircle) / 2;

    /// Formatos aceitos como avatar (GIF fica de fora).
    private static readonly FilePickerFileType AvatarImageTypes = new("Imagens")
    {
        Patterns = new[] { "*.png", "*.jpg", "*.jpeg", "*.bmp", "*.webp" },
    };

    private Border _avatarButton = null!, _avatarEditor = null!, _avatarDropZone = null!, _avatarCropArea = null!;
    private Ellipse _avatarImage = null!;
    private Image _avatarCropImage = null!;
    private Slider _avatarZoomSlider = null!;
    private Grid _avatarZoomRow = null!;
    private TextBlock _avatarEditorError = null!;
    private Button _avatarChooseButton = null!, _avatarRemoveButton = null!, _avatarCancelButton = null!,
        _avatarSaveButton = null!, _settingsButton = null!;

    private bool _avatarEditorOpen;

    // Imagem sendo recortada e a posição dela dentro da área de recorte
    private Bitmap? _cropSource;
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
        _avatarSaveButton.Click += (_, _) => SaveAvatar();
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

        ApplyAvatar(AvatarStore.Load());
    }

    private void ApplyAvatar(Bitmap? avatar)
    {
        _avatarImage.Fill = avatar is null
            ? new SolidColorBrush(Color.Parse("#2E343C"))
            : new ImageBrush(avatar) { Stretch = Stretch.UniformToFill };
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
        SetAvatarError(null);
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
        _cropDragFrom = null;

        _avatarDropZone.IsVisible = true;
        _avatarCropArea.IsVisible = false;
        _avatarZoomRow.IsVisible = false;
        _avatarChooseButton.IsVisible = false;
        _avatarSaveButton.IsEnabled = false;
    }

    private void SetAvatarError(string? message)
    {
        _avatarEditorError.Text = message ?? "";
        _avatarEditorError.IsVisible = !string.IsNullOrEmpty(message);
    }

    private async Task PickAvatarFileAsync()
    {
        var files = await StorageProvider.OpenFilePickerAsync(new FilePickerOpenOptions
        {
            Title = "Escolha uma imagem para o avatar",
            AllowMultiple = false,
            FileTypeFilter = new[] { AvatarImageTypes },
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

            if (AvatarStore.IsGif(bytes))
            {
                SetAvatarError("GIF não é aceito como avatar. Use PNG, JPG, BMP ou WEBP.");
                return;
            }

            var bitmap = new Bitmap(new MemoryStream(bytes));
            if (bitmap.PixelSize.Width < 2 || bitmap.PixelSize.Height < 2)
            {
                bitmap.Dispose();
                SetAvatarError("Essa imagem é pequena demais.");
                return;
            }

            SetAvatarError(null);
            StartCrop(bitmap);
        }
        catch (Exception)
        {
            SetAvatarError("Não foi possível abrir essa imagem. Use PNG, JPG, BMP ou WEBP.");
        }
    }

    // ───────────────────────── Recorte ─────────────────────────

    /// Mostra a imagem na área de recorte, cobrindo o círculo e centralizada.
    private void StartCrop(Bitmap bitmap)
    {
        _cropSource?.Dispose();
        _cropSource = bitmap;
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

    private void SaveAvatar()
    {
        if (_cropSource is null) return;

        // O pedaço da imagem (em pixels) que está dentro do círculo
        var crop = new Rect(
            (CropInset - _cropX) / _cropScale,
            (CropInset - _cropY) / _cropScale,
            CropCircle / _cropScale,
            CropCircle / _cropScale);

        try
        {
            AvatarStore.Save(_cropSource, crop);
        }
        catch (Exception ex)
        {
            SetAvatarError("Não foi possível salvar o avatar: " + ex.Message);
            return;
        }

        ApplyAvatar(AvatarStore.Load());
        CloseAvatarEditor();
    }

    private void RemoveAvatar()
    {
        try
        {
            AvatarStore.Remove();
        }
        catch (Exception ex)
        {
            SetAvatarError("Não foi possível remover o avatar: " + ex.Message);
            return;
        }

        ApplyAvatar(AvatarStore.LoadDefault());
        CloseAvatarEditor();
    }
}