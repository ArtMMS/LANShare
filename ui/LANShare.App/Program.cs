using Avalonia;

namespace LANShare.App;

internal static class Program
{
    // O Avalonia exige que o ponto de entrada rode na thread principal (STA no Windows).
    [STAThread]
    public static void Main(string[] args)
    {
        // O idioma precisa estar definido antes de qualquer tela ser montada
        Loc.Init(AppPreferences.Load().Language);
        BuildAvaloniaApp().StartWithClassicDesktopLifetime(args);
    }

    public static AppBuilder BuildAvaloniaApp() =>
        AppBuilder.Configure<App>()
            .UsePlatformDetect()
            .LogToTrace();
}
