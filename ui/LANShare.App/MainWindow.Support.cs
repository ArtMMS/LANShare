using System.Diagnostics;
using Avalonia.Input;
using Avalonia.Interactivity;

namespace LANShare.App;

public partial class MainWindow
{
    /// Página do projeto (clicar no título "LANShare").
    private const string ProjectUrl = "https://github.com/ArtMMS/LANShare";

    /// Seção de apoio (botões "Apoie o LANShare").
    private const string SupportUrl = "https://github.com/ArtMMS/LANShare#-support-lanshare";

    /// Clique no título "LANShare": abre a página do projeto.
    private void OnTitlePointerPressed(object? sender, PointerPressedEventArgs e)
    {
        if (!e.GetCurrentPoint(this).Properties.IsLeftButtonPressed) return;

        OpenLink(ProjectUrl);
        e.Handled = true;
    }

    /// Botões "Apoie o LANShare": abrem a seção de apoio.
    private void OnSupportClick(object? sender, RoutedEventArgs e) => OpenLink(SupportUrl);

    /// Abre o link no navegador padrão.
    private void OpenLink(string url)
    {
        try
        {
            Process.Start(new ProcessStartInfo(url) { UseShellExecute = true });
        }
        catch (Exception ex)
        {
            _infoText.Text = Loc.F("Link_OpenError", ex.Message);
        }
    }
}
