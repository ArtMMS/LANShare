using Avalonia.Threading;

namespace LANShare.App;

/// Atualizações automáticas (V0.5).
/// Etapa 1: checa ao abrir e mostra o resultado num aviso (provisório; vira o cartão na Etapa 2).
public partial class MainWindow
{
    /// Espera um pouco depois de abrir, para não atrasar a abertura do app.
    private static readonly TimeSpan StartupCheckDelay = TimeSpan.FromSeconds(3);

    private const string UpdateGlyph = "\uE895";

    private async Task CheckForUpdatesOnStartupAsync()
    {
        if (!_preferences.AutoCheckUpdates) return;

        await Task.Delay(StartupCheckDelay);
        var result = await UpdateChecker.CheckAsync();

        Dispatcher.UIThread.Post(() => ShowUpdateResult(result));
    }

    /// Provisório: mostra o resultado da checagem num aviso.
    private void ShowUpdateResult(UpdateCheckResult result)
    {
        switch (result.Status)
        {
            case UpdateStatus.UpdateAvailable:
                var release = result.Release!;
                var asset = release.DownloadUrl is null ? " (sem o LANShare.exe anexado)" : "";
                Notify($"Nova versão disponível: {release.Tag}{asset}. Você usa {AppInfo.DisplayVersion}.", UpdateGlyph);
                break;

            case UpdateStatus.UpToDate:
                Notify($"Você está na versão mais recente ({AppInfo.DisplayVersion}).", UpdateGlyph);
                break;

            default:
                Notify("Não foi possível verificar atualizações: " + result.Error, UpdateGlyph);
                break;
        }
    }
}