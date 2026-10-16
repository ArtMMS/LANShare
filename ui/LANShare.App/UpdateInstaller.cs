using System.Diagnostics;
using System.Net.Http.Headers;
using System.Security.Cryptography;

namespace LANShare.App;

/// Erro de atualização com uma mensagem pronta para mostrar ao usuário.
internal sealed class UpdateException(string message, Exception? inner = null) : Exception(message, inner);

/// Baixa a versão nova, troca o LANShare.exe e reinicia.
/// A troca é por renomeação: o Windows não deixa sobrescrever um exe em uso, mas deixa renomear.
internal static class UpdateInstaller
{
    /// Argumento passado ao exe novo, para ele avisar "Atualizado para vX".
    private const string UpdatedArgument = "--updated";

    private static readonly HttpClient Http = CreateClient();

    private static HttpClient CreateClient()
    {
        var client = new HttpClient { Timeout = TimeSpan.FromMinutes(10) };
        client.DefaultRequestHeaders.UserAgent.Add(new ProductInfoHeaderValue("LANShare", AppInfo.VersionText));
        return client;
    }

    /// Caminho do exe em execução.
    private static string? ExePath => Environment.ProcessPath;

    /// Só o LANShare.exe publicado (arquivo único) pode se substituir.
    /// No `dotnet run` o exe é o da pasta de compilação, e trocar ele não faz sentido.
#pragma warning disable IL3000 // Location vazio é justamente o sinal de "arquivo único"
    public static bool CanSelfUpdate =>
        string.IsNullOrEmpty(typeof(UpdateInstaller).Assembly.Location)
        && ExePath is { } path
        && path.EndsWith(".exe", StringComparison.OrdinalIgnoreCase);
#pragma warning restore IL3000

    /// O app acabou de ser aberto pelo atualizador?
    public static bool JustUpdated =>
        Environment.GetCommandLineArgs().Skip(1).Any(a => a == UpdatedArgument);

    /// Baixa o LANShare.exe da release para "LANShare.exe.new" e confere. Devolve o caminho do arquivo.
    public static async Task<string> DownloadAsync(
        ReleaseInfo release, IProgress<(long Received, long Total)> progress, CancellationToken cancel = default)
    {
        if (release.DownloadUrl is null || ExePath is null)
            throw new UpdateException("Esta versão não tem o LANShare.exe para baixar.");

        var target = ExePath + ".new";
        TryDelete(target);

        try
        {
            using var response = await Http.GetAsync(release.DownloadUrl, HttpCompletionOption.ResponseHeadersRead, cancel);
            if (!response.IsSuccessStatusCode)
                throw new UpdateException($"O GitHub respondeu com erro {(int)response.StatusCode}.");

            var total = response.Content.Headers.ContentLength ?? release.DownloadSize;
            using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
            long received = 0;

            await using (var source = await response.Content.ReadAsStreamAsync(cancel))
            await using (var file = new FileStream(target, FileMode.Create, FileAccess.Write, FileShare.None, 81920, useAsync: true))
            {
                var buffer = new byte[81920];
                int read;
                while ((read = await source.ReadAsync(buffer, cancel)) > 0)
                {
                    await file.WriteAsync(buffer.AsMemory(0, read), cancel);
                    hash.AppendData(buffer, 0, read);
                    received += read;
                    progress.Report((received, total));
                }
            }

            Verify(target, release, received, hash.GetHashAndReset());
            return target;
        }
        catch (Exception ex) when (ex is not OperationCanceledException || !cancel.IsCancellationRequested)
        {
            TryDelete(target); // nunca deixa um arquivo pela metade
            throw Friendly(ex);
        }
    }

    /// Troca o exe atual pelo baixado e abre a versão nova. Quem chama fecha o app em seguida.
    public static void InstallAndRestart(string downloadedPath)
    {
        var exe = ExePath ?? throw new UpdateException("Não foi possível localizar o LANShare.exe.");
        var old = exe + ".old";

        try
        {
            TryDelete(old);
            File.Move(exe, old, overwrite: true); // renomear o exe em uso é permitido
            try
            {
                File.Move(downloadedPath, exe);
            }
            catch
            {
                File.Move(old, exe, overwrite: true); // desfaz: o app atual volta ao lugar
                throw;
            }
        }
        catch (UnauthorizedAccessException ex)
        {
            throw new UpdateException("Sem permissão para substituir o LANShare.exe nesta pasta.", ex);
        }
        catch (IOException ex)
        {
            throw new UpdateException("Não foi possível substituir o LANShare.exe: " + ex.Message, ex);
        }

        try
        {
            Process.Start(new ProcessStartInfo(exe, UpdatedArgument)
            {
                UseShellExecute = false,
                WorkingDirectory = Path.GetDirectoryName(exe)!,
            });
        }
        catch (Exception ex)
        {
            throw new UpdateException("A versão nova foi instalada, mas não abriu sozinha. Abra o LANShare de novo.", ex);
        }
    }

    /// Ao abrir: apaga o exe antigo (.old) e downloads que ficaram pela metade (.new).
    public static void CleanupLeftovers()
    {
        if (ExePath is not { } exe) return;

        TryDelete(exe + ".new");

        // O app antigo pode ainda estar fechando: tenta por até 10 s
        var old = exe + ".old";
        _ = Task.Run(async () =>
        {
            for (var attempt = 0; attempt < 20 && File.Exists(old); attempt++)
            {
                if (TryDelete(old)) return;
                await Task.Delay(500);
            }
        });
    }

    private static void Verify(string path, ReleaseInfo release, long received, byte[] sha256)
    {
        if (release.DownloadSize > 0 && received != release.DownloadSize)
            throw new UpdateException("O arquivo baixado está incompleto.");

        if (release.Sha256 is { } expected
            && !Convert.ToHexString(sha256).Equals(expected, StringComparison.OrdinalIgnoreCase))
            throw new UpdateException("O arquivo baixado está corrompido (hash diferente).");

        // Todo programa do Windows começa com "MZ"
        using var file = File.OpenRead(path);
        Span<byte> head = stackalloc byte[2];
        if (file.Read(head) != 2 || head[0] != (byte)'M' || head[1] != (byte)'Z')
            throw new UpdateException("O arquivo baixado não é um programa válido.");
    }

    private static UpdateException Friendly(Exception ex) => ex switch
    {
        UpdateException update => update,
        OperationCanceledException => new UpdateException("O download demorou demais.", ex),
        HttpRequestException => new UpdateException("Sem conexão com o GitHub.", ex),
        UnauthorizedAccessException => new UpdateException("Sem permissão para gravar na pasta do LANShare.", ex),
        IOException => new UpdateException("Erro ao salvar o arquivo: " + ex.Message, ex),
        _ => new UpdateException(ex.Message, ex),
    };

    private static bool TryDelete(string path)
    {
        try
        {
            if (File.Exists(path)) File.Delete(path);
            return true;
        }
        catch (Exception)
        {
            return false;
        }
    }
}