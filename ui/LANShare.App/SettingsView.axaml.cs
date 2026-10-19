using Avalonia;
using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.Layout;
using Avalonia.Markup.Xaml;
using Avalonia.Media;
using Avalonia.Platform.Storage;

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
        _checkUpdatesButton, _restartButton;
    private readonly ToggleSwitch _soundJoinLeaveSwitch, _soundStreamSwitch, _autoUpdateSwitch;
    private readonly TextBlock _updateStatusText, _restartText, _audioDeviceHint;
    private readonly ComboBox _audioDeviceBox;
    private readonly Grid _restartRow;

    // Apps que não transmitem som
    private readonly StackPanel _excludedAppsPanel, _audioAppsPanel, _excludedAppsContent;
    private readonly Button _addAudioAppButton, _browseExeButton, _excludedAppsHeader;
    private readonly TextBlock _excludedAppsChevron;
    private readonly Border _audioAppsBox;
    private readonly TextBlock _audioAppsStatus;

    /// Muda a cada busca de apps com som: uma resposta atrasada é ignorada.
    private int _audioAppsRequest;

    /// Botões de idioma, na mesma ordem de Loc.All.
    private readonly Button[] _languageButtons;

    private AppPreferences _prefs = new();
    private bool _loading;

    /// true = preenchendo a lista de saídas de áudio (a seleção não conta como mudança).
    private bool _loadingAudio;

    /// Muda a cada listagem: uma resposta atrasada de uma abertura anterior é ignorada.
    private int _audioRequest;

    public event EventHandler? CloseRequested;
    public event EventHandler? PreferencesChanged;

    /// Botão "Verificar agora" (a janela faz a checagem e devolve o resultado em SetUpdateCheckState).
    public event EventHandler? CheckNowRequested;

    /// Botão "Reiniciar agora" (aparece depois de trocar o idioma).
    public event EventHandler? RestartRequested;

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
        _restartButton = Find<Button>("RestartButton");
        _restartText = Find<TextBlock>("RestartText");
        _restartRow = Find<Grid>("RestartRow");
        _audioDeviceBox = Find<ComboBox>("AudioDeviceBox");
        _audioDeviceHint = Find<TextBlock>("AudioDeviceHint");
        _languageButtons = new[] { Find<Button>("LangPtButton"), Find<Button>("LangEnButton"), Find<Button>("LangEsButton") };

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

        // Saída do som: só "Padrão do Windows" até a lista chegar do Rust
        _audioDeviceBox.ItemsSource = new List<ComboBoxItem> { DefaultDeviceItem(null) };
        _audioDeviceBox.SelectedIndex = 0;
        _audioDeviceBox.SelectionChanged += (_, _) => OnAudioDeviceChanged();

        // Idioma: salva na hora, mas só vale depois de reiniciar o app
        for (var i = 0; i < _languageButtons.Length; i++)
        {
            var language = Loc.All[i];
            _languageButtons[i].Click += (_, _) => Change(p => p.Language = Loc.CodeOf(language));
        }
        _restartButton.Click += (_, _) => RestartRequested?.Invoke(this, EventArgs.Empty);

        // Apps que não transmitem som
        _excludedAppsPanel = Find<StackPanel>("ExcludedAppsPanel");
        _audioAppsPanel = Find<StackPanel>("AudioAppsPanel");
        _addAudioAppButton = Find<Button>("AddAudioAppButton");
        _browseExeButton = Find<Button>("BrowseExeButton");
        _audioAppsBox = Find<Border>("AudioAppsBox");
        _audioAppsStatus = Find<TextBlock>("AudioAppsStatus");
        _excludedAppsContent = Find<StackPanel>("ExcludedAppsContent");
        _excludedAppsHeader = Find<Button>("ExcludedAppsHeader");
        _excludedAppsChevron = Find<TextBlock>("ExcludedAppsChevron");

        // O cartão começa recolhido: clicar no título abre (e fecha) as opções
        _excludedAppsHeader.Click += (_, _) => SetExcludedAppsOpen(!_excludedAppsContent.IsVisible);

        _addAudioAppButton.Click += (_, _) =>
        {
            if (_audioAppsBox.IsVisible) _audioAppsBox.IsVisible = false;
            else ShowAudioApps();
        };
        _browseExeButton.Click += async (_, _) => await BrowseExeAsync();
    }

    /// Idioma escolhido nas preferências (sem escolha = o que está em uso).
    private AppLanguage ChosenLanguage => Loc.FromCode(_prefs.Language) ?? Loc.Current;

    /// Mensagem na linha do reinício (ex.: "feche a sala antes"). Null = o aviso padrão.
    internal void SetRestartMessage(string? message)
    {
        _restartText.Text = message ?? Loc.T("Settings_RestartPending");
    }

    private T Find<T>(string name) where T : Control => this.FindControl<T>(name)!;

    /// Mostra as preferências (o mesmo objeto que a janela usa).
    internal void Load(AppPreferences prefs)
    {
        _prefs = prefs;
        IsCapturingKey = false;
        _shortcutButton.Classes.Set("capturing", false);
        SetExcludedAppsOpen(false); // toda vez que as Configurações abrem, o cartão vem recolhido
        Refresh();
        RefreshAudioDevices();
    }

    /// Resultado do "Verificar agora". `busy` = verificando (botão desativado).
    internal void SetUpdateCheckState(bool busy, string? status)
    {
        _checkUpdatesButton.IsEnabled = !busy;
        _checkUpdatesButton.Content = Loc.T(busy ? "Update_Checking" : "Settings_CheckNow");
        _updateStatusText.Text = status ?? "";
        _updateStatusText.IsVisible = !string.IsNullOrEmpty(status);
    }

    /// Nome da tecla como aparece no botão.
    internal static string KeyLabel(Key? key)
    {
        if (key is not { } k) return Loc.T("Key_None");

        return k switch
        {
            >= Key.D0 and <= Key.D9 => ((int)k - (int)Key.D0).ToString(),
            >= Key.NumPad0 and <= Key.NumPad9 => $"Num {(int)k - (int)Key.NumPad0}",
            Key.Space => Loc.T("Key_Space"),
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

        var chosen = ChosenLanguage;
        for (var i = 0; i < _languageButtons.Length; i++)
            _languageButtons[i].Classes.Set("selected", Loc.All[i] == chosen);

        // Escolheu outro idioma: pede reinício. Voltou ao atual: o aviso some.
        var pending = chosen != Loc.Current;
        if (pending && !_restartRow.IsVisible) SetRestartMessage(null);
        _restartRow.IsVisible = pending;

        RefreshExcludedApps();

        _loading = false;
    }

    // ───────────────────────── Saída do áudio ─────────────────────────

    /// Lista as saídas de áudio do PC (fora da thread da interface) e marca a que está salva.
    private async void RefreshAudioDevices()
    {
        var request = ++_audioRequest;
        _audioDeviceBox.IsEnabled = false;

        List<AudioOutputDevice> devices;
        string? error;
        try
        {
            (devices, error) = await Task.Run(() => NativeMethods.AudioOutputDevices());
        }
        catch (Exception ex)
        {
            (devices, error) = (new List<AudioOutputDevice>(), ex.Message);
        }

        // O painel foi reaberto enquanto listava: vale a listagem mais nova
        if (request != _audioRequest) return;

        var items = new List<ComboBoxItem> { DefaultDeviceItem(devices.FirstOrDefault(d => d.IsDefault)) };
        items.AddRange(devices.Select(d => new ComboBoxItem { Content = d.Name, Tag = d.Id }));

        var saved = _prefs.AudioOutputDevice;
        var selected = saved is null ? null : items.FirstOrDefault(i => i.Tag as string == saved);

        _loadingAudio = true;
        _audioDeviceBox.ItemsSource = items;
        _audioDeviceBox.SelectedItem = selected ?? items[0];
        _loadingAudio = false;
        _audioDeviceBox.IsEnabled = true;

        if (error is not null) SetAudioHint(Loc.T("Settings_AudioDevicesError"), warning: true);
        else if (saved is not null && selected is null) SetAudioHint(Loc.T("Settings_AudioDeviceMissing"), warning: true);
        else SetAudioHint(Loc.T("Settings_AudioOutputHint"), warning: false);
    }

    /// Primeira opção da lista: segue o padrão do Windows (mostra qual é ele agora, se souber).
    private static ComboBoxItem DefaultDeviceItem(AudioOutputDevice? current) => new()
    {
        Content = current is null
            ? Loc.T("Settings_WindowsDefault")
            : Loc.F("Settings_WindowsDefaultNamed", current.Name),
        Tag = null, // null = padrão do Windows
    };

    /// O usuário escolheu outra saída: salva na hora (o padrão do Windows é salvo como null).
    private void OnAudioDeviceChanged()
    {
        if (_loadingAudio || _audioDeviceBox.SelectedItem is not ComboBoxItem item) return;

        var id = item.Tag as string;
        if (id == _prefs.AudioOutputDevice) return;

        Change(p => p.AudioOutputDevice = id);
        SetAudioHint(Loc.T("Settings_AudioOutputHint"), warning: false);
    }

    // ───────────────────────── Apps que não transmitem som ─────────────────────────

    /// Abre ou fecha o cartão "Apps que não transmitem som" (fechado, some também a lista de "Adicionar app").
    private void SetExcludedAppsOpen(bool open)
    {
        _excludedAppsContent.IsVisible = open;
        _excludedAppsChevron.Text = open ? "" : ""; // seta para cima / para baixo
        if (!open) _audioAppsBox.IsVisible = false;
    }

    /// Uma linha por app: nome, os .exe, o interruptor (ligado = não transmite) e,
    /// para os adicionados pelo usuário, o botão de remover.
    private void RefreshExcludedApps()
    {
        _excludedAppsPanel.Children.Clear();

        foreach (var app in _prefs.ExcludedAudioApps)
        {
            var text = new StackPanel { Spacing = 1, VerticalAlignment = VerticalAlignment.Center };
            text.Children.Add(new TextBlock { Text = app.Name, TextTrimming = TextTrimming.CharacterEllipsis });
            text.Children.Add(new TextBlock
            {
                Text = string.Join(", ", app.Exes),
                FontSize = 11,
                Opacity = 0.5,
                TextTrimming = TextTrimming.CharacterEllipsis,
            });

            var toggle = new ToggleSwitch
            {
                IsChecked = app.Enabled,
                OnContent = "",
                OffContent = "",
                MinWidth = 0,
                VerticalAlignment = VerticalAlignment.Center,
            };
            toggle.IsCheckedChanged += (_, _) => Change(_ => app.Enabled = toggle.IsChecked == true);
            Grid.SetColumn(toggle, 1);

            var row = new Grid { ColumnDefinitions = new ColumnDefinitions("*,Auto,Auto") };
            row.Children.Add(text);
            row.Children.Add(toggle);

            if (!app.BuiltIn)
            {
                var remove = new Button
                {
                    Content = new TextBlock { Classes = { "icon" }, Text = "", FontSize = 13 },
                    Background = Brushes.Transparent,
                    Padding = new Thickness(8, 6),
                    Margin = new Thickness(6, 0, 0, 0),
                    VerticalAlignment = VerticalAlignment.Center,
                };
                ToolTip.SetTip(remove, Loc.T("Settings_RemoveApp"));
                remove.Click += (_, _) => Change(p => p.ExcludedAudioApps.Remove(app));
                Grid.SetColumn(remove, 2);
                row.Children.Add(remove);
            }

            _excludedAppsPanel.Children.Add(new Border
            {
                Background = new SolidColorBrush(Color.Parse("#1B1F24")),
                CornerRadius = new CornerRadius(8),
                Padding = new Thickness(12, 8),
                Child = row,
            });
        }
    }

    /// "Adicionar app": lista os apps com som abertos agora (fora da thread da interface).
    private async void ShowAudioApps()
    {
        var request = ++_audioAppsRequest;
        _audioAppsBox.IsVisible = true;
        _audioAppsPanel.Children.Clear();
        SetAudioAppsStatus(Loc.T("Settings_AudioAppsLoading"));

        List<AudioApp> apps;
        string? error;
        try
        {
            (apps, error) = await Task.Run(() => NativeMethods.AudioApps());
        }
        catch (Exception ex)
        {
            (apps, error) = (new List<AudioApp>(), ex.Message);
        }

        if (request != _audioAppsRequest || !_audioAppsBox.IsVisible) return;

        if (error is not null)
        {
            SetAudioAppsStatus(Loc.F("Settings_AudioAppsError", error));
            return;
        }
        if (apps.Count == 0)
        {
            SetAudioAppsStatus(Loc.T("Settings_AudioAppsEmpty"));
            return;
        }

        SetAudioAppsStatus(null);
        foreach (var app in apps)
        {
            var inList = ExcludedAudioApp.Contains(_prefs.ExcludedAudioApps, app.Exe);
            var detail = inList ? Loc.T("Settings_AppInList")
                : app.Active ? $"{app.Exe} · {Loc.T("Settings_AppPlayingNow")}"
                : app.Exe;

            var content = new StackPanel { Spacing = 1 };
            content.Children.Add(new TextBlock { Text = app.Name });
            content.Children.Add(new TextBlock { Text = detail, FontSize = 11, Opacity = 0.55 });

            var button = new Button
            {
                Content = content,
                IsEnabled = !inList,
                HorizontalAlignment = HorizontalAlignment.Stretch,
                HorizontalContentAlignment = HorizontalAlignment.Left,
                Padding = new Thickness(10, 6),
            };
            button.Click += (_, _) => AddExcludedApp(app.Name, app.Exe);
            _audioAppsPanel.Children.Add(button);
        }
    }

    /// "Escolher .exe…": adiciona um programa escolhido no disco.
    private async Task BrowseExeAsync()
    {
        if (TopLevel.GetTopLevel(this)?.StorageProvider is not { } storage) return;

        var files = await storage.OpenFilePickerAsync(new FilePickerOpenOptions
        {
            Title = Loc.T("Settings_ExePickerTitle"),
            AllowMultiple = false,
            FileTypeFilter = new[] { new FilePickerFileType(Loc.T("Settings_ExeFileType")) { Patterns = new[] { "*.exe" } } },
        });
        if (files.Count == 0) return;

        var exe = files[0].Name;
        if (!exe.EndsWith(".exe", StringComparison.OrdinalIgnoreCase)) return;

        var stem = Path.GetFileNameWithoutExtension(exe);
        var name = stem.Length > 0 ? char.ToUpper(stem[0]) + stem[1..] : exe;
        AddExcludedApp(name, exe);
    }

    /// Põe o app no fim da lista, já ligado (se ainda não estiver nela).
    private void AddExcludedApp(string name, string exe)
    {
        if (ExcludedAudioApp.Contains(_prefs.ExcludedAudioApps, exe)) return;

        _audioAppsBox.IsVisible = false;
        Change(p => p.ExcludedAudioApps.Add(ExcludedAudioApp.Custom(name, exe)));
    }

    private void SetAudioAppsStatus(string? text)
    {
        _audioAppsStatus.Text = text ?? "";
        _audioAppsStatus.IsVisible = !string.IsNullOrEmpty(text);
    }

    private void SetAudioHint(string text, bool warning)
    {
        _audioDeviceHint.Text = text;
        _audioDeviceHint.Classes.Set("warning", warning);
    }

    // ───────────────────────── Captura da tecla ─────────────────────────

    private void StartCapture()
    {
        IsCapturingKey = true;
        _shortcutButton.Content = Loc.T("Settings_PressKey");
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