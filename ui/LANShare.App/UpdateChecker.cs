using System.Net;
using System.Net.Http.Headers;
using System.Text.Json;

namespace LANShare.App;

/// A última release publicada no GitHub.
internal sealed record ReleaseInfo(
    Version Version,
    string Tag,
    string PageUrl,
    string? Notes,
    string? DownloadUrl, // null = a release não tem o LANShare.exe anexado
    long DownloadSize,
    string? Sha256);     // hash informado pelo GitHub (pode não vir)

internal enum UpdateStatus { UpToDate, UpdateAvailable, Failed }

internal sealed record UpdateCheckResult(UpdateStatus Status, ReleaseInfo? Release, string? Error);

/// Consulta a última release do LANShare no GitHub e compara com a versão instalada.
internal static class UpdateChecker
{
    private const string LatestReleaseUrl = "https://api.github.com/repos/ArtMMS/LANShare/releases/latest";

    /// Nome exato do arquivo que cada release deve ter anexado.
    public const string AssetName = "LANShare.exe";

    private static readonly HttpClient Http = CreateClient();

    private static HttpClient CreateClient()
    {
        var client = new HttpClient { Timeout = TimeSpan.FromSeconds(10) };
        // A API do GitHub recusa pedidos sem User-Agent
        client.DefaultRequestHeaders.UserAgent.Add(new ProductInfoHeaderValue("LANShare", AppInfo.VersionText));
        client.DefaultRequestHeaders.Accept.Add(new MediaTypeWithQualityHeaderValue("application/vnd.github+json"));
        client.DefaultRequestHeaders.Add("X-GitHub-Api-Version", "2022-11-28");
        return client;
    }

    /// Pergunta ao GitHub qual é a última versão estável e compara com a instalada. Nunca lança erro de rede.
    public static async Task<UpdateCheckResult> CheckAsync(CancellationToken cancel = default)
    {
        try
        {
            using var response = await Http.GetAsync(LatestReleaseUrl, cancel);

            // O "latest" ignora pré-releases e rascunhos: 404 = nenhuma versão estável
            if (response.StatusCode == HttpStatusCode.NotFound)
                return Failed(Loc.T("UpdateErr_NoStable"));
            if (response.StatusCode is HttpStatusCode.Forbidden or HttpStatusCode.TooManyRequests)
                return Failed(Loc.T("UpdateErr_RateLimit"));
            if (!response.IsSuccessStatusCode)
                return Failed(Loc.F("UpdateErr_HttpStatus", (int)response.StatusCode));

            await using var stream = await response.Content.ReadAsStreamAsync(cancel);
            using var doc = await JsonDocument.ParseAsync(stream, cancellationToken: cancel);

            var release = ParseRelease(doc.RootElement);
            if (release is null) return Failed(Loc.T("UpdateErr_BadTag"));

            var status = release.Version > AppInfo.Version ? UpdateStatus.UpdateAvailable : UpdateStatus.UpToDate;
            return new UpdateCheckResult(status, release, null);
        }
        catch (OperationCanceledException) when (!cancel.IsCancellationRequested)
        {
            return Failed(Loc.T("UpdateErr_Timeout"));
        }
        catch (HttpRequestException)
        {
            return Failed(Loc.T("UpdateErr_NoConnection"));
        }
        catch (JsonException)
        {
            return Failed(Loc.T("UpdateErr_BadResponse"));
        }
    }

    /// "v0.5.1", "0.5.1", "v1.0-beta" -> 0.5.1 / 1.0.0. Null se não for uma versão.
    public static Version? ParseTag(string? tag)
    {
        if (string.IsNullOrWhiteSpace(tag)) return null;

        var text = tag.Trim().TrimStart('v', 'V');
        var cut = text.IndexOfAny(new[] { '-', '+', ' ' });
        if (cut >= 0) text = text[..cut]; // ignora sufixos (-beta, +build)
        if (!text.Contains('.')) text += ".0"; // "1" -> "1.0"

        return Version.TryParse(text, out var version) ? AppInfo.Normalize(version) : null;
    }

    private static ReleaseInfo? ParseRelease(JsonElement root)
    {
        var tag = GetString(root, "tag_name");
        var version = ParseTag(tag);
        if (version is null) return null;

        string? downloadUrl = null, sha256 = null;
        long size = 0;

        if (root.TryGetProperty("assets", out var assets) && assets.ValueKind == JsonValueKind.Array)
        {
            foreach (var asset in assets.EnumerateArray())
            {
                if (!string.Equals(GetString(asset, "name"), AssetName, StringComparison.OrdinalIgnoreCase)) continue;

                downloadUrl = GetString(asset, "browser_download_url");
                size = asset.TryGetProperty("size", out var s) && s.TryGetInt64(out var n) ? n : 0;

                // Formato do GitHub: "sha256:abc123..."
                var digest = GetString(asset, "digest");
                if (digest is not null && digest.StartsWith("sha256:", StringComparison.OrdinalIgnoreCase))
                    sha256 = digest["sha256:".Length..];
                break;
            }
        }

        return new ReleaseInfo(
            version,
            tag!,
            GetString(root, "html_url") ?? "https://github.com/ArtMMS/LANShare/releases",
            GetString(root, "body"),
            downloadUrl,
            size,
            sha256);
    }

    private static string? GetString(JsonElement element, string name) =>
        element.TryGetProperty(name, out var value) && value.ValueKind == JsonValueKind.String
            ? value.GetString()
            : null;

    private static UpdateCheckResult Failed(string error) => new(UpdateStatus.Failed, null, error);
}
