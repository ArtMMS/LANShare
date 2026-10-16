using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Threading;

namespace LANShare.App;

/// Atualizações (V0.5): checagem ao abrir, "Verificar agora", cartão na lateral,
/// download, instalação e reinício.
public partial class MainWindow
{
    /// Espera um pouco depois de abrir, para não atrasar a abertura do app.
    private static readonly TimeSpan StartupCheckDelay = TimeSpan.FromSeconds(3);

    private const string UpdateGlyph = "\uE895";

    /// Em que ponto está a atualização mostrada no cartão.
    private enum UpdatePhase { Available, Downloading, ReadyToInstall, Installing }

    private Border _updateCard = null!;
    private TextBlock _updateTitle = null!, _updateText = null!, _updateNotesLink = null!;
    private ProgressBar _updateProgress = null!;
    private Grid _updateButtons = null!;
    private Button _updateNowButton = null!, _updateLaterButton = null!;

    // Confere a cada meio segundo se dá para atualizar (sala aberta / conectado bloqueia)
    private readonly DispatcherTimer _updateCardTimer = new() { Interval = TimeSpan.FromMilliseconds(500) };

    private ReleaseInfo? _availableRelease;
    private UpdatePhase _updatePhase = UpdatePhase.Available;
    private string? _downloadedPath;
    private string? _updateError;
    private bool _checkingUpdates;

    /// Chamado no construtor: prepara o cartão, limpa sobras de uma atualização anterior
    /// e, se estiver ligado nas configurações, checa ao abrir.
    private async Task CheckForUpdatesOnStartupAsync()
    {
        InitUpdates();
        UpdateInstaller.CleanupLeftovers();

        if (UpdateInstaller.JustUpdated)
        {
            // Um pouco depois, para a janela já estar na tela
            DispatcherTimer.RunOnce(
                () => Notify($"LANShare atualizado para {AppInfo.DisplayVersion}.", UpdateGlyph),
                TimeSpan.FromSeconds(1));
        }

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
        _updateCard.IsVisible = true;
        _updateCardTimer.Start();

        // Baixando ou instalando: não mexe no que está em andamento
        if (_updatePhase is UpdatePhase.Downloading or UpdatePhase.Installing) return;

        // Versão diferente da que foi baixada antes: começa do zero
        if (_availableRelease is null || _availableRelease.Version != release.Version)
        {
            _updatePhase = UpdatePhase.Available;
            _downloadedPath = null;
        }

        _availableRelease = release;
        _updateError = null;
        RefreshUpdateCard();
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

        // Baixando ou instalando: só a barra de progresso (título e texto vêm do progresso)
        if (_updatePhase is UpdatePhase.Downloading or UpdatePhase.Installing)
        {
            _updateButtons.IsVisible = false;
            _updateNotesLink.IsVisible = false;
            _updateProgress.IsVisible = true;
            return;
        }

        string text;
        if (_updatePhase == UpdatePhase.ReadyToInstall)
        {
            _updateTitle.Text = "Pronto para instalar";
            _updateNowButton.Content = "Instalar";
            text = $"{release.Tag} já foi baixada. O LANShare vai reiniciar.";
        }
        else
        {
            _updateTitle.Text = "Nova versão disponível";
            _updateNowButton.Content = "Atualizar";
            text = $"{release.Tag} está disponível. Você usa {AppInfo.DisplayVersion}.";
        }

        if (_updateError is not null) text += "\n" + _updateError;
        if (UpdateBlocked) text += "\nFeche a sala ou desconecte para atualizar.";

        _updateText.Text = text;
        _updateProgress.IsVisible = false;
        _updateButtons.IsVisible = true;
        _updateNotesLink.IsVisible = true;
        _updateNowButton.IsEnabled = !UpdateBlocked;
    }

    // ───────────────────────── Baixar, instalar e reiniciar ─────────────────────────

    /// Botão "Atualizar" (ou "Instalar", se já foi baixada).
    private async void StartUpdate()
    {
        if (_availableRelease is not { } release || UpdateBlocked) return;

        if (_updatePhase == UpdatePhase.ReadyToInstall && _downloadedPath is { } ready)
        {
            Install(ready);
            return;
        }
        if (_updatePhase != UpdatePhase.Available) return;

        // Release sem o exe anexado: atualização manual pela página
        if (release.DownloadUrl is null)
        {
            OpenLink(release.PageUrl);
            return;
        }

        // Rodando pelo `dotnet run`: não há um LANShare.exe publicado para trocar
        if (!UpdateInstaller.CanSelfUpdate)
        {
            Notify("A instalação automática só funciona no LANShare.exe publicado. Abrindo a página da versão.", UpdateGlyph);
            OpenLink(release.PageUrl);
            return;
        }

        _updatePhase = UpdatePhase.Downloading;
        _updateError = null;
        _updateTitle.Text = $"Baixando {release.Tag}...";
        _updateText.Text = "Iniciando o download...";
        _updateProgress.Value = 0;
        _updateProgress.IsIndeterminate = release.DownloadSize <= 0;
        RefreshUpdateCard();

        // Progress<T> devolve cada atualização na thread da interface
        var progress = new Progress<(long Received, long Total)>(p =>
        {
            if (p.Total > 0)
            {
                _updateProgress.IsIndeterminate = false;
                _updateProgress.Value = p.Received * 100.0 / p.Total;
                _updateText.Text = $"{Megabytes(p.Received)} de {Megabytes(p.Total)} ({p.Received * 100 / p.Total}%)";
            }
            else
            {
                _updateText.Text = $"{Megabytes(p.Received)} baixados";
            }
        });

        try
        {
            _downloadedPath = await UpdateInstaller.DownloadAsync(release, progress);
        }
        catch (Exception ex)
        {
            _updatePhase = UpdatePhase.Available;
            _updateError = "Falha no download: " + ex.Message;
            RefreshUpdateCard();
            return;
        }

        _updatePhase = UpdatePhase.ReadyToInstall;

        // Abriu uma sala durante o download: espera o usuário clicar em "Instalar"
        if (UpdateBlocked)
        {
            RefreshUpdateCard();
            return;
        }

        Install(_downloadedPath);
    }

    private void Install(string downloadedPath)
    {
        _updatePhase = UpdatePhase.Installing;
        _updateError = null;
        _updateTitle.Text = "Instalando...";
        _updateText.Text = "O LANShare vai reiniciar.";
        _updateProgress.IsIndeterminate = true;
        RefreshUpdateCard();

        try
        {
            UpdateInstaller.InstallAndRestart(downloadedPath);
        }
        catch (Exception ex)
        {
            _updatePhase = UpdatePhase.Available;
            _downloadedPath = null;
            _updateProgress.IsIndeterminate = false;
            _updateError = "Falha ao instalar: " + ex.Message;
            RefreshUpdateCard();
            return;
        }

        // A versão nova já está abrindo: fecha esta
        Close();
    }

    private static string Megabytes(long bytes) => $"{bytes / 1024.0 / 1024.0:F1} MB";
}