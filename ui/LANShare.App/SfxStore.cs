using System.Globalization;
using System.Text;
using System.Text.Json;

namespace LANShare.App;

/// Um efeito sonoro da biblioteca do usuário.
internal sealed class SfxEntry
{
    /// Identificador (também é o nome do arquivo, sem a extensão).
    public string Id { get; set; } = "";

    /// Nome mostrado no painel (pode ter emoji colado pelo usuário).
    public string Name { get; set; } = "";

    /// Nome do arquivo dentro da pasta sfx (ex.: "3f2a...c1.mp3").
    public string FileName { get; set; } = "";

    public SfxEntry Copy() => new() { Id = Id, Name = Name, FileName = FileName };
}

/// Erro de efeito sonoro com uma mensagem pronta para mostrar ao usuário.
internal sealed class SfxException(string message, Exception? inner = null) : Exception(message, inner);

/// Biblioteca de efeitos sonoros: os arquivos ficam em %APPDATA%\LANShare\sfx
/// e o índice sfx.json guarda o nome e a ordem de cada um.
/// O arquivo é guardado como veio (.mp3, .wav ou .ogg); ele só é validado ao ser adicionado.
internal static class SfxStore
{
    /// Tamanho máximo do arquivo ao adicionar (um .wav de 5 s em 48 kHz tem ~1 MB).
    public const long MaxFileBytes = 2 * 1024 * 1024;

    /// Tamanho máximo do nome, em caracteres "visíveis" (um emoji conta como um).
    public const int MaxNameLength = 32;

    private static readonly JsonSerializerOptions JsonOptions = new() { WriteIndented = true };

    /// O índice é lido e gravado por mais de uma thread (adicionar roda fora da interface).
    private static readonly object Gate = new();
    private static List<SfxEntry>? _entries;

    private static string Folder =>
        Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "LANShare", "sfx");

    private static string IndexPath => Path.Combine(Folder, "sfx.json");

    private sealed class SfxIndex
    {
        public List<SfxEntry> Sounds { get; set; } = new();
    }

    /// Lista lida do disco na primeira vez; depois fica em memória.
    private static List<SfxEntry> Entries => _entries ??= LoadIndex();

    /// Cópia da biblioteca, na ordem do painel.
    public static List<SfxEntry> All()
    {
        lock (Gate) return Entries.Select(e => e.Copy()).ToList();
    }

    /// Caminho completo do arquivo de um som.
    public static string PathOf(SfxEntry entry) => Path.Combine(Folder, entry.FileName);

    /// Valida (formato, duração, tamanho) e guarda um som novo no fim da lista.
    /// Demora alguns milissegundos (decodifica o arquivo inteiro): chame fora da thread da interface.
    public static SfxEntry Import(byte[] data, string originalName)
    {
        if (data.Length > MaxFileBytes) throw new SfxException(Loc.T("Sfx_TooBig"));

        // O formato vem do conteúdo, não da extensão (um .mp3 renomeado para .wav ainda funciona)
        var format = SfxDecoder.Detect(data) ?? throw new SfxException(Loc.T("Sfx_Unsupported"));
        SfxDecoder.Decode(data, format); // lança SfxException se for longo demais ou inválido

        var name = CleanName(DefaultName(originalName));
        if (name.Length == 0) name = Loc.T("Sfx_DefaultName");

        lock (Gate)
        {
            var id = Guid.NewGuid().ToString("N");
            var entry = new SfxEntry
            {
                Id = id,
                Name = name,
                FileName = id + SfxDecoder.Extension(format),
            };

            Directory.CreateDirectory(Folder);
            var path = PathOf(entry);
            var temp = path + ".tmp";
            try
            {
                // Grava num temporário e troca: nunca fica um arquivo pela metade
                File.WriteAllBytes(temp, data);
                File.Move(temp, path, overwrite: true);
            }
            catch
            {
                TryDelete(temp);
                throw;
            }

            Entries.Add(entry);
            try
            {
                SaveIndex();
            }
            catch
            {
                // Sem índice, o arquivo ficaria "perdido" na pasta: desfaz tudo
                Entries.Remove(entry);
                TryDelete(path);
                throw;
            }

            return entry.Copy();
        }
    }

    /// Troca o nome de um som. False se o nome novo ficou vazio (o antigo continua).
    public static bool Rename(string id, string? newName)
    {
        var name = CleanName(newName);
        if (name.Length == 0) return false;

        lock (Gate)
        {
            var entry = Entries.FirstOrDefault(e => e.Id == id);
            if (entry is null || entry.Name == name) return false;

            var old = entry.Name;
            entry.Name = name;
            try
            {
                SaveIndex();
            }
            catch
            {
                entry.Name = old;
                throw;
            }
            return true;
        }
    }

    /// Tira um som da biblioteca e apaga o arquivo dele.
    public static void Remove(string id)
    {
        string path;
        lock (Gate)
        {
            var index = Entries.FindIndex(e => e.Id == id);
            if (index < 0) return;

            var entry = Entries[index];
            Entries.RemoveAt(index);
            try
            {
                SaveIndex();
            }
            catch
            {
                Entries.Insert(index, entry);
                throw;
            }
            path = PathOf(entry);
        }

        TryDelete(path);
        SfxPlayer.Forget(path);
    }

    /// Limpa um nome: sem caracteres de controle, sem espaços nas pontas e no máximo
    /// MaxNameLength caracteres visíveis (não corta um emoji no meio).
    public static string CleanName(string? text)
    {
        var filtered = new string((text ?? "").Where(c => !char.IsControl(c)).ToArray()).Trim();

        var result = new StringBuilder();
        var elements = StringInfo.GetTextElementEnumerator(filtered);
        var count = 0;
        while (count < MaxNameLength && elements.MoveNext())
        {
            result.Append(elements.GetTextElement());
            count++;
        }
        return result.ToString().Trim();
    }

    /// Nome inicial: o nome do arquivo, sem extensão e com "_" virando espaço.
    private static string DefaultName(string? fileName) =>
        Path.GetFileNameWithoutExtension(fileName ?? "").Replace('_', ' ').Trim();

    /// Lê o índice. Ausente ou quebrado = biblioteca vazia (os arquivos na pasta não são apagados).
    private static List<SfxEntry> LoadIndex()
    {
        try
        {
            if (!File.Exists(IndexPath)) return new List<SfxEntry>();

            var index = JsonSerializer.Deserialize<SfxIndex>(File.ReadAllText(IndexPath), JsonOptions);
            var seen = new HashSet<string>();
            var list = new List<SfxEntry>();

            foreach (var entry in index?.Sounds ?? new List<SfxEntry>())
            {
                // Só aceita um nome de arquivo simples (o índice pode ter sido editado à mão)
                if (string.IsNullOrWhiteSpace(entry.Id) || !seen.Add(entry.Id)) continue;
                if (string.IsNullOrWhiteSpace(entry.FileName) || Path.GetFileName(entry.FileName) != entry.FileName) continue;
                if (!File.Exists(Path.Combine(Folder, entry.FileName))) continue;

                entry.Name = CleanName(entry.Name);
                if (entry.Name.Length == 0) entry.Name = Loc.T("Sfx_DefaultName");
                list.Add(entry);
            }
            return list;
        }
        catch (Exception)
        {
            return new List<SfxEntry>();
        }
    }

    /// Grava o índice (num temporário e troca, para não corromper se falhar no meio).
    private static void SaveIndex()
    {
        Directory.CreateDirectory(Folder);
        var temp = IndexPath + ".tmp";
        File.WriteAllText(temp, JsonSerializer.Serialize(new SfxIndex { Sounds = Entries }, JsonOptions));
        File.Move(temp, IndexPath, overwrite: true);
    }

    private static void TryDelete(string path)
    {
        try
        {
            if (File.Exists(path)) File.Delete(path);
        }
        catch (Exception)
        {
            // em uso ou sem permissão: fica para a próxima
        }
    }
}
