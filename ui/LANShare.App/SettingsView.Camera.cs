using System.Runtime.InteropServices;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Media.Imaging;
using Avalonia.Platform;
using Avalonia.Threading;

namespace LANShare.App;

/// Configurações -> Câmera: qual câmera, com prévia ao vivo, e a resolução (até 4K) e o FPS
/// (até 60) — as listas mostram só o que a câmera escolhida suporta. Cada escolha é salva na hora
/// e vale na próxima vez que a câmera for ligada na sala.
///
/// A prévia liga a câmera só enquanto as Configurações estão abertas. Se a câmera já estiver
/// ligada na sala, a prévia não abre (o meu quadrado da sala já mostra a imagem).
public partial class SettingsView
{
    /// Maior resolução e FPS oferecidos (o resto da câmera fica de fora da lista).
    private const int CameraMaxHeight = 2160;
    private const int CameraMaxFps = 60;

    /// Intervalo entre as imagens da prévia (~30 FPS).
    private static readonly TimeSpan CameraPreviewInterval = TimeSpan.FromMilliseconds(33);

    private ComboBox _cameraDeviceBox = null!, _cameraResolutionBox = null!, _cameraFpsBox = null!;
    private Image _cameraPreviewImage = null!;
    private TextBlock _cameraPreviewText = null!, _cameraHint = null!;

    private readonly DispatcherTimer _cameraPreviewTimer = new();

    /// A prévia vem em até 360p (folga para câmeras bem largas).
    private readonly byte[] _cameraTestBuffer = new byte[1280 * 360 * 4];
    private WriteableBitmap? _cameraTestBitmap;

    /// Os formatos da câmera escolhida.
    private List<CameraModeInfo> _cameraModes = new();

    /// true = preenchendo as listas (a seleção não conta como mudança).
    private bool _loadingCamera;

    /// Muda a cada pedido: respostas atrasadas (de uma câmera ou abertura anterior) são ignoradas.
    private int _cameraRequest;

    /// O cartão está ativo (Configurações abertas)?
    private bool _cameraSectionActive;

    /// A câmera está ligada na sala (a prévia não abre).
    private bool _cameraInUse;

    /// Chamado uma vez, no construtor.
    private void InitCamera()
    {
        _cameraDeviceBox = Find<ComboBox>("CameraDeviceBox");
        _cameraResolutionBox = Find<ComboBox>("CameraResolutionBox");
        _cameraFpsBox = Find<ComboBox>("CameraFpsBox");
        _cameraPreviewImage = Find<Image>("CameraPreviewImage");
        _cameraPreviewText = Find<TextBlock>("CameraPreviewText");
        _cameraHint = Find<TextBlock>("CameraHint");

        _cameraDeviceBox.SelectionChanged += (_, _) => OnCameraDeviceChanged();
        _cameraResolutionBox.SelectionChanged += (_, _) => OnCameraResolutionChanged();
        _cameraFpsBox.SelectionChanged += (_, _) => OnCameraFpsChanged();

        _cameraPreviewTimer.Interval = CameraPreviewInterval;
        _cameraPreviewTimer.Tick += (_, _) => UpdateCameraPreview();
    }

    /// As Configurações abriram: lista as câmeras e liga a prévia.
    /// `cameraInUse` = a câmera já está ligada na sala.
    internal async void StartCameraSection(bool cameraInUse)
    {
        _cameraSectionActive = true;
        _cameraInUse = cameraInUse;
        var request = ++_cameraRequest;

        SetCameraBoxesEnabled(false);
        ShowCameraText(Loc.T("Settings_CameraSearching"));
        _cameraHint.Text = Loc.T("Settings_CameraHint");

        List<CameraDeviceInfo> cameras;
        string? error;
        try
        {
            (cameras, error) = await Task.Run(() => NativeMethods.CameraDevices());
        }
        catch (Exception ex)
        {
            (cameras, error) = (new List<CameraDeviceInfo>(), ex.Message);
        }

        if (request != _cameraRequest || !_cameraSectionActive) return;

        if (cameras.Count == 0)
        {
            _loadingCamera = true;
            _cameraDeviceBox.ItemsSource = null;
            _cameraResolutionBox.ItemsSource = null;
            _cameraFpsBox.ItemsSource = null;
            _loadingCamera = false;
            ShowCameraText(error is null ? Loc.T("Settings_CameraNone") : Loc.F("Settings_CameraError", error));
            return;
        }

        var items = cameras.Select(c => new ComboBoxItem { Content = c.Name, Tag = c.Id }).ToList();
        var saved = items.FirstOrDefault(i => i.Tag as string == _prefs.CameraDeviceId);

        _loadingCamera = true;
        _cameraDeviceBox.ItemsSource = items;
        _cameraDeviceBox.SelectedItem = saved ?? items[0];
        _loadingCamera = false;
        _cameraDeviceBox.IsEnabled = true;

        await LoadCameraModes(request);
    }

    /// As Configurações fecharam: desliga a câmera da prévia.
    internal void StopCameraSection()
    {
        if (!_cameraSectionActive) return;

        _cameraSectionActive = false;
        _cameraRequest++; // respostas que ainda vão chegar são ignoradas
        _cameraPreviewTimer.Stop();
        ClearCameraPreview();
        _ = Task.Run(NativeMethods.CameraTestStop);
    }

    /// A câmera escolhida na lista (null = nenhuma).
    private string? SelectedCameraId => (_cameraDeviceBox.SelectedItem as ComboBoxItem)?.Tag as string;

    private int? SelectedCameraHeight => (_cameraResolutionBox.SelectedItem as ComboBoxItem)?.Tag as int?;
    private int? SelectedCameraFps => (_cameraFpsBox.SelectedItem as ComboBoxItem)?.Tag as int?;

    /// Lê os formatos da câmera escolhida, preenche resolução e FPS e liga a prévia.
    private async Task LoadCameraModes(int request)
    {
        var deviceId = SelectedCameraId;
        SetCameraBoxesEnabled(false);
        _cameraDeviceBox.IsEnabled = true;

        // A prévia anterior sai antes: a mesma câmera não abre duas vezes
        _cameraPreviewTimer.Stop();
        ClearCameraPreview();
        ShowCameraText(Loc.T("Settings_CameraStarting"));
        await Task.Run(NativeMethods.CameraTestStop);

        List<CameraModeInfo> modes;
        string? error;
        try
        {
            (modes, error) = await Task.Run(() => NativeMethods.CameraModes(deviceId));
        }
        catch (Exception ex)
        {
            (modes, error) = (new List<CameraModeInfo>(), ex.Message);
        }

        if (request != _cameraRequest || !_cameraSectionActive) return;

        _cameraModes = modes.Where(m => m.Height <= CameraMaxHeight && m.Fps <= CameraMaxFps).ToList();
        if (error is not null) _cameraHint.Text = Loc.F("Settings_CameraModesError", error);

        FillCameraResolutions();
        FillCameraFps();
        SetCameraBoxesEnabled(true);

        await RestartCameraPreview(request);
    }

    /// Resoluções que a câmera tem (sem repetir), da maior para a menor; marca a salva
    /// (ou a maior até ela).
    private void FillCameraResolutions()
    {
        var heights = _cameraModes.Select(m => m.Height).Distinct().OrderByDescending(h => h).ToList();
        var items = heights.Select(h => new ComboBoxItem { Content = ResolutionLabel(h), Tag = h }).ToList();

        var wanted = _prefs.CameraHeight;
        var pick = heights.Contains(wanted) ? wanted
            : heights.Where(h => h <= wanted).DefaultIfEmpty(heights.LastOrDefault()).First();

        _loadingCamera = true;
        _cameraResolutionBox.ItemsSource = items;
        _cameraResolutionBox.SelectedItem = items.FirstOrDefault(i => (int)i.Tag! == pick);
        _loadingCamera = false;
    }

    /// FPS que a câmera tem na resolução escolhida, do maior para o menor; marca o salvo
    /// (ou o maior até ele).
    private void FillCameraFps()
    {
        var height = SelectedCameraHeight;
        var fpsList = _cameraModes.Where(m => m.Height == height)
            .Select(m => m.Fps).Distinct().OrderByDescending(f => f).ToList();
        var items = fpsList.Select(f => new ComboBoxItem { Content = $"{f} FPS", Tag = f }).ToList();

        var wanted = _prefs.CameraFps;
        var pick = fpsList.Contains(wanted) ? wanted
            : fpsList.Where(f => f <= wanted).DefaultIfEmpty(fpsList.LastOrDefault()).First();

        _loadingCamera = true;
        _cameraFpsBox.ItemsSource = items;
        _cameraFpsBox.SelectedItem = items.FirstOrDefault(i => (int)i.Tag! == pick);
        _loadingCamera = false;
    }

    private static string ResolutionLabel(int height) => height switch
    {
        2160 => "2160p (4K)",
        1440 => "1440p (QHD)",
        _ => $"{height}p",
    };

    private async void OnCameraDeviceChanged()
    {
        if (_loadingCamera || !_cameraSectionActive) return;

        var id = SelectedCameraId;
        if (id != _prefs.CameraDeviceId) Change(p => p.CameraDeviceId = id);
        await LoadCameraModes(++_cameraRequest);
    }

    private async void OnCameraResolutionChanged()
    {
        if (_loadingCamera || !_cameraSectionActive || SelectedCameraHeight is not { } height) return;

        Change(p => p.CameraHeight = height);
        FillCameraFps(); // o FPS depende da resolução
        if (SelectedCameraFps is { } fps && fps != _prefs.CameraFps) Change(p => p.CameraFps = fps);
        await RestartCameraPreview(++_cameraRequest);
    }

    private async void OnCameraFpsChanged()
    {
        if (_loadingCamera || !_cameraSectionActive || SelectedCameraFps is not { } fps) return;

        Change(p => p.CameraFps = fps);
        await RestartCameraPreview(++_cameraRequest);
    }

    /// Liga (de novo) a câmera da prévia com a câmera, a resolução e o FPS escolhidos.
    private async Task RestartCameraPreview(int request)
    {
        _cameraPreviewTimer.Stop();
        ClearCameraPreview();

        if (_cameraInUse)
        {
            ShowCameraText(Loc.T("Settings_CameraInUse"));
            return;
        }

        ShowCameraText(Loc.T("Settings_CameraStarting"));

        var deviceId = SelectedCameraId;
        var height = (uint)(SelectedCameraHeight ?? _prefs.CameraHeight);
        var fps = (uint)(SelectedCameraFps ?? _prefs.CameraFps);

        (bool Ok, string? Error) result;
        try
        {
            result = await Task.Run(() => NativeMethods.CameraTestStart(deviceId, height, fps));
        }
        catch (Exception ex)
        {
            result = (false, ex.Message);
        }

        // Fechou as Configurações enquanto a câmera abria: desliga de novo
        if (!_cameraSectionActive)
        {
            _ = Task.Run(NativeMethods.CameraTestStop);
            return;
        }
        if (request != _cameraRequest) return; // um pedido mais novo vai cuidar da prévia

        if (!result.Ok)
        {
            ShowCameraText(Loc.F("Settings_CameraError", result.Error ?? "?"));
            return;
        }

        _cameraPreviewTimer.Start();
    }

    /// Pega a imagem mais recente da prévia e desenha.
    private void UpdateCameraPreview()
    {
        if (!NativeMethods.CameraTestFrame(_cameraTestBuffer, out var width, out var height)) return;

        if (_cameraTestBitmap is null
            || _cameraTestBitmap.PixelSize.Width != width
            || _cameraTestBitmap.PixelSize.Height != height)
        {
            _cameraTestBitmap = new WriteableBitmap(
                new PixelSize(width, height), new Vector(96, 96), PixelFormat.Bgra8888, AlphaFormat.Opaque);
            _cameraPreviewImage.Source = _cameraTestBitmap;
        }

        using (var frame = _cameraTestBitmap.Lock())
        {
            for (var y = 0; y < height; y++)
                Marshal.Copy(_cameraTestBuffer, y * width * 4, frame.Address + y * frame.RowBytes, width * 4);
        }

        _cameraPreviewImage.InvalidateVisual();
        _cameraPreviewImage.IsVisible = true;
        _cameraPreviewText.IsVisible = false;
    }

    private void ShowCameraText(string text)
    {
        _cameraPreviewText.Text = text;
        _cameraPreviewText.IsVisible = true;
        _cameraPreviewImage.IsVisible = false;
    }

    private void ClearCameraPreview()
    {
        _cameraPreviewImage.Source = null;
        _cameraPreviewImage.IsVisible = false;
        _cameraTestBitmap = null;
    }

    private void SetCameraBoxesEnabled(bool enabled)
    {
        _cameraDeviceBox.IsEnabled = enabled;
        _cameraResolutionBox.IsEnabled = enabled;
        _cameraFpsBox.IsEnabled = enabled;
    }
}
