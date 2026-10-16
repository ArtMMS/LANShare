using System.Reflection;

namespace LANShare.App;

/// Versão do app, lida do próprio exe (vem do <Version> do LANShare.App.csproj).
internal static class AppInfo
{
    /// Sempre com 3 partes: X.Y.Z.
    public static Version Version { get; } = ReadVersion();

    /// "0.5.0"
    public static string VersionText => $"{Version.Major}.{Version.Minor}.{Version.Build}";

    /// "v0.5.0" (como aparece no título e nas tags do GitHub)
    public static string DisplayVersion => "v" + VersionText;

    private static Version ReadVersion()
    {
        var version = Assembly.GetEntryAssembly()?.GetName().Version
                      ?? typeof(AppInfo).Assembly.GetName().Version;
        return version is null ? new Version(0, 0, 0) : Normalize(version);
    }

    /// Descarta a 4ª parte (revision) e garante X.Y.Z.
    public static Version Normalize(Version v) => new(v.Major, v.Minor, Math.Max(v.Build, 0));
}