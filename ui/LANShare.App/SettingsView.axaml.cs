using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.Markup.Xaml;

namespace LANShare.App;

/// Painel de configurações (aberto pela engrenagem). Cada mudança é salva na hora.
public partial class SettingsView : UserControl
{
    /// Teclas que sozinhas não viram atalho (espera a tecla "de verdade").
    private static readonly Key[] ModifierOnlyKeys =
    {
        Key.LeftCtrl, Key.RightCtrl, Key.LeftShift, Key.RightShift,
        Key.LeftAlt, Key.RightAlt, Key.LWin, Key.RWin,
    };

    private readonly Button _closeButton, _shortcutButton, _res1080Button, _res720Button, _fps30Button, _fps15Button,
        _checkUpdatesButton;
    private readonly ToggleSwitch _soundJoinLeaveSwitch, _soundStreamSwitch, _autoUpdateSwitch;
    private readonly TextBlock _updateStatusText;

    private AppPreferences _prefs = new();
    private bool _loading;

    public event EventHandler? CloseRequested;
    public event EventHandler? PreferencesChanged;

    /// Botão "Verificar agora" (a janela faz a checagem e devolve o resultado em SetUpdateCheckState).
    public event EventHandler? CheckNowRequested;

    /// true = esperando o usuário apertar a nova tecla do atalho.
    public bool IsCapturingKey { get; private set; }

    public SettingsView()
    {
        AvaloniaXamlLoader.Load(this);

        _closeButton = Find<Button>("CloseButton");
        _shortcutButton = Find<Button>("ShortcutButton");
        _res1080Button = Find<Button>("Res1080Button");
        _res720Button = Find<Button>("Res720Button");
        _fps30Button = Find<Button>("Fps30Button");
        _fps15Button = Find<Button>("Fps15Button");
        _checkUpdatesButton = Find<Button>("CheckUpdatesButton");
        _soundJoinLeaveSwitch = Find<ToggleSwitch>("SoundJoinLeaveSwitch");
        _soundStreamSwitch = Find<ToggleSwitch>("SoundStreamSwitch");
        _autoUpdateSwitch = Find<ToggleSwitch>("AutoUpdateSwitch");
        _updateStatusText = Find<TextBlock>("UpdateStatusText");

        _closeButton.Click += (_, _) => CloseRequested?.Invoke(this, EventArgs.Empty);

        // Atalho: clicar começa a esperar a tecla; sair do botão cancela
        _shortcutButton.Click += (_, _) => StartCapture();
        _shortcutButton.AddHandler(KeyDownEvent, OnShortcutKeyDown, RoutingStrategies.Tunnel);
        _shortcutButton.LostFocus += (_, _) => CancelCapture();

        _res1080Button.Click += (_, _) => Change(p => p.StreamHeight = 1080);
        _res720Button.Click += (_, _) => Change(p => p.StreamHeight = 720);
        _fps30Button.Click += (_, _) => Change(p => p.StreamFps = 30);
        _fps15Button.Click += (_, _) => Change(p => p.StreamFps = 15);

        _soundJoinLeaveSwitch.IsCheckedChanged += (_, _) =>
            Change(p => p.SoundOnUserJoinLeave = _soundJoinLeaveSwitch.IsChecked == true);
        _soundStreamSwitch.IsCheckedChanged += (_, _) =>
            Change(p => p.SoundOnStreamStart = _soundStreamSwitch.IsChecked == true);
        _autoUpdateSwitch.IsCheckedChanged += (_, _) =>
            Change(p => p.AutoCheckUpdates = _autoUpdateSwitch.IsChecked == true);

        _checkUpdatesButton.Click += (_, _) => CheckNowRequested?.Invoke(this, EventArgs.Empty);
    }

    private T Find<T>(string name) where T : Control => this.FindControl<T>(name)!;

    /// Mostra as preferências (o mesmo objeto que a janela usa).
    internal void Load(AppPreferences prefs)
    {
        _prefs = prefs;
        IsCapturingKey = false;
        _shortcutButton.Classes.Set("capturing", false);
        Refresh();
    }

    /// Resultado do "Verificar agora". `busy` = verificando (botão desativado).
    internal void SetUpdateCheckState(bool busy, string? status)
    {
        _checkUpdatesButton.IsEnabled = !busy;
        _checkUpdatesButton.Content = busy ? "Verificando..." : "Verificar agora";
        _updateStatusText.Text = status ?? "";
        _updateStatusText.IsVisible = !string.IsNullOrEmpty(status);
    }

    /// Nome da tecla como aparece no botão.
    internal static string KeyLabel(Key? key)
    {
        if (key is not { } k) return "Nenhuma";

        return k switch
        {
            >= Key.D0 and <= Key.D9 => ((int)k - (int)Key.D0).ToString(),
            >= Key.NumPad0 and <= Key.NumPad9 => $"Num {(int)k - (int)Key.NumPad0}",
            Key.Space => "Espaço",
            Key.Enter => "Enter",
            Key.Back => "Backspace",
            Key.PageUp => "Page Up",
            Key.PageDown => "Page Down",
            _ => k.ToString(),
        };
    }

    /// Aplica a mudança, salva e avisa a janela.
    private void Change(Action<AppPreferences> apply)
    {
        if (_loading) return;
        apply(_prefs);
        _prefs.Save();
        Refresh();
        PreferencesChanged?.Invoke(this, EventArgs.Empty);
    }

    private void Refresh()
    {
        _loading = true; // mexer nos interruptores aqui não conta como mudança

        if (!IsCapturingKey) _shortcutButton.Content = KeyLabel(_prefs.FullscreenKeyValue);

        _res1080Button.Classes.Set("selected", _prefs.StreamHeight == 1080);
        _res720Button.Classes.Set("selected", _prefs.StreamHeight == 720);
        _fps30Button.Classes.Set("selected", _prefs.StreamFps == 30);
        _fps15Button.Classes.Set("selected", _prefs.StreamFps == 15);

        _soundJoinLeaveSwitch.IsChecked = _prefs.SoundOnUserJoinLeave;
        _soundStreamSwitch.IsChecked = _prefs.SoundOnStreamStart;
        _autoUpdateSwitch.IsChecked = _prefs.AutoCheckUpdates;

        _loading = false;
    }

    // ───────────────────────── Captura da tecla ─────────────────────────

    private void StartCapture()
    {
        IsCapturingKey = true;
        _shortcutButton.Content = "Pressione uma tecla...";
        _shortcutButton.Classes.Set("capturing", true);
    }

    /// Para de esperar a tecla, sem mudar o atalho.
    internal void CancelCapture()
    {
        if (!IsCapturingKey) return;
        IsCapturingKey = false;
        _shortcutButton.Classes.Set("capturing", false);
        _shortcutButton.Content = KeyLabel(_prefs.FullscreenKeyValue);
    }

    private void OnShortcutKeyDown(object? sender, KeyEventArgs e)
    {
        if (!IsCapturingKey) return;
        e.Handled = true; // a tecla não chega ao botão (Enter/Espaço não "clicam" de novo)

        if (e.Key == Key.Escape)
        {
            FinishCapture(null); // Esc = sem atalho
            return;
        }
        if (e.Key == Key.None || ModifierOnlyKeys.Contains(e.Key)) return;

        FinishCapture(e.Key);
    }

    private void FinishCapture(Key? key)
    {
        IsCapturingKey = false;
        _shortcutButton.Classes.Set("capturing", false);
        Change(p => p.FullscreenKeyValue = key);
    }
}