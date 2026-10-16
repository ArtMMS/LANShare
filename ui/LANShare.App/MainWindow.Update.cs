using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Threading;

namespace LANShare.App;

/// Atualizações (V0.5): checagem ao abrir, "Verificar agora" e o cartão de aviso na lateral.
public partial class MainWindow
{
    /// Espera um pouco depois de abrir, para não atrasar a abertura do app.
    private static readonly TimeSpan StartupCheckDelay = TimeSpan.FromSeconds(3);

    private const string UpdateGlyph = "\uE895";

    private Border _updateCard = null!;
    private TextBlock _updateTitle = null!, _updateText = null!, _updateNotesLink = null!;
    private ProgressBar _updateProgress = null!;
    private Grid _updateButtons = null!;
    private Button _updateNowButton = null!, _updateLaterButton = null!;

    // Confere a cada meio segundo se dá para atualizar (sala aberta / conectado bloqueia)
    private readonly DispatcherTimer _updateCardTimer = new() { Interval = TimeSpan.FromMilliseconds(500) };

    private ReleaseInfo? _availableRelease;
    private bool _checkingUpdates;

    /// Chamado no construtor: prepara o cartão e, se estiver ligado nas configurações, checa ao abrir.
    private async Task CheckForUpdatesOnStartupAsync()
    {
        InitUpdates();
        if (!_preferences.AutoCheckUpdates) return;

        await Task.Delay(StartupCheckDelay);
        var result = await RunUpdateCheckAsync();

        // Automático: erro ou "já atualizado" ficam em silêncio
        if (result is { Status: UpdateStatus.UpdateAvailable, Release: { } release })
        {
            ShowUpdateCard(release);
            Notify($"Nova versão disponível: {release.Tag}.", UpdateGlyph);
        }
    }

    private void InitUpdates()
    {
        _updateCard = Find<Border>("UpdateCard");
        _updateTitle = Find<TextBlock>("UpdateTitle");
        _updateText = Find<TextBlock>("UpdateText");
        _updateNotesLink = Find<TextBlock>("UpdateNotesLink");
        _updateProgress = Find<ProgressBar>("UpdateProgress");
        _updateButtons = Find<Grid>("UpdateButtons");
        _updateNowButton = Find<Button>("UpdateNowButton");
        _updateLaterButton = Find<Button>("UpdateLaterButton");

        _updateLaterButton.Click += (_, _) => HideUpdateCard(); // volta na próxima abertura do app
        _updateNowButton.Click += (_, _) => StartUpdate();
        _updateNotesLink.PointerPressed += (_, e) =>
        {
            if (!e.GetCurrentPoint(_updateNotesLink).Properties.IsLeftButtonPressed) return;
            if (_availableRelease is { } release) OpenLink(release.PageUrl);
            e.Handled = true;
        };

        _updateCardTimer.Tick += (_, _) => RefreshUpdateCard();

        // Botão "Verificar agora" das configurações
        _settingsView.CheckNowRequested += (_, _) => _ = CheckNowAsync();
    }

    /// "Verificar agora": funciona mesmo com a checagem automática desligada e sempre mostra o resultado.
    private async Task CheckNowAsync()
    {
        _settingsView.SetUpdateCheckState(busy: true, "Verificando...");

        var result = await RunUpdateCheckAsync();
        if (result is null)
        {
            _settingsView.SetUpdateCheckState(busy: false, "Já existe uma verificação em andamento. Tente de novo em instantes.");
            return;
        }

        string message;
        switch (result.Status)
        {
            case UpdateStatus.UpdateAvailable:
                ShowUpdateCard(result.Release!);
                message = $"Nova versão disponível: {result.Release!.Tag}. Veja o aviso na lateral.";
                break;
            case UpdateStatus.UpToDate:
                message = $"Você está na versão mais recente ({AppInfo.DisplayVersion}).";
                break;
            default:
                message = "Não foi possível verificar: " + result.Error;
                break;
        }

        _settingsView.SetUpdateCheckState(busy: false, message);
    }

    /// Consulta o GitHub (uma checagem por vez). Null = já havia uma em andamento.
    private async Task<UpdateCheckResult?> RunUpdateCheckAsync()
    {
        if (_checkingUpdates) return null;
        _checkingUpdates = true;
        try
        {
            return await UpdateChecker.CheckAsync();
        }
        finally
        {
            _checkingUpdates = false;
        }
    }

    // ───────────────────────── Cartão na lateral ─────────────────────────

    private void ShowUpdateCard(ReleaseInfo release)
    {
        _availableRelease = release;
        _updateTitle.Text = "Nova versão disponível";
        _updateProgress.IsVisible = false;
        _updateButtons.IsVisible = true;
        _updateNotesLink.IsVisible = true;
        _updateCard.IsVisible = true;

        RefreshUpdateCard();
        _updateCardTimer.Start();
    }

    private void HideUpdateCard()
    {
        _updateCard.IsVisible = false;
        _updateCardTimer.Stop();
    }

    /// Com sala aberta ou conectado, não dá para atualizar (derrubaria todo mundo).
    private bool UpdateBlocked => _roomOpen || _clientState != ClientState.Idle;

    private void RefreshUpdateCard()
    {
        if (_availableRelease is not { } release) return;

        var text = $"{release.Tag} está disponível. Você usa {AppInfo.DisplayVersion}.";
        if (UpdateBlocked) text += "\nFeche a sala ou desconecte para atualizar.";

        _updateText.Text = text;
        _updateNowButton.IsEnabled = !UpdateBlocked;
    }

    /// Botão "Atualizar".
    /// Provisório (Etapa 2): abre a página da release. Na Etapa 3, baixa e instala aqui.
    private void StartUpdate()
    {
        if (_availableRelease is not { } release || UpdateBlocked) return;
        OpenLink(release.PageUrl);
    }
}