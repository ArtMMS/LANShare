using Avalonia;

namespace LANShare.App;

internal static class Program
{
    // O Avalonia exige que o ponto de entrada rode na thread principal (STA no Windows).
    [STAThread]
    public static void Main(string[] args) =>
        BuildAvaloniaApp().StartWithClassicDesktopLifetime(args);

    public static AppBuilder BuildAvaloniaApp() =>
        AppBuilder.Configure<App>()
            .UsePlatformDetect()
            .LogToTrace();
}