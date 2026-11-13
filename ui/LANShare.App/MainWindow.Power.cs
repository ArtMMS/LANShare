using Avalonia.Threading;

namespace LANShare.App;

/// Mantém a tela ligada e o PC acordado enquanto você assiste ou transmite
/// (como o YouTube faz), e libera assim que não precisa mais.
public partial class MainWindow
{
    /// De quanto em quanto tempo confere se o pedido de energia ainda é necessário.
    /// O Windows só apaga a tela depois de minutos: 1 s de diferença não importa.
    private static readonly TimeSpan PowerCheckInterval = TimeSpan.FromSeconds(1);

    /// Texto que aparece no `powercfg /requests`.
    private const string PowerRequestReason = "LANShare: watching or sharing a screen";

    private readonly DispatcherTimer _powerTimer = new();
    private PowerRequest? _powerRequest;

    /// Precisa manter a tela ligada? Assistindo (ou conectando para assistir) ou transmitindo.
    /// Ao sair da sala, fechar a sala ou ser expulso, os dois estados são zerados.
    private bool KeepAwake => _streaming || _watchState != WatchState.None;

    /// Chamado uma vez no construtor.
    private void InitPowerRequest()
    {
        _powerRequest = PowerRequest.TryCreate(PowerRequestReason);

        _powerTimer.Interval = PowerCheckInterval;
        _powerTimer.Tick += (_, _) => _powerRequest?.Set(KeepAwake);
        _powerTimer.Start();

        // Ao fechar o app, libera na hora (se o app fechar à força, o Windows libera sozinho)
        Closing += (_, _) =>
        {
            _powerTimer.Stop();
            _powerRequest?.Dispose();
            _powerRequest = null;
        };
    }
}