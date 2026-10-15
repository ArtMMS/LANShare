using Avalonia;
using Avalonia.Media.Imaging;
using Avalonia.Platform;

namespace LANShare.App;

/// Avatar do usuário: fica em %APPDATA%\LANShare\avatar.png (já recortado).
/// Sem arquivo salvo, usa o avatar padrão que vem dentro do app.
internal static class AvatarStore
{
    /// Lado da imagem salva, em pixels.
    public const int Size = 256;

    private static readonly Uri DefaultAvatarUri = new("avares://LANShare/Assets/default_avatar.jpg");

    private static string Folder =>
        Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "LANShare");

    private static string PngPath => Path.Combine(Folder, "avatar.png");

    // Sobras da versão de testes com GIF: são apagadas ao salvar ou remover
    private static string OldGifPath => Path.Combine(Folder, "avatar.gif");
    private static string OldCropPath => Path.Combine(Folder, "avatar_crop.json");

    /// Existe um avatar escolhido pelo usuário?
    public static bool HasCustom => File.Exists(PngPath);

    /// Os bytes são de um GIF? (começa com "GIF") — GIF não é aceito como avatar.
    public static bool IsGif(byte[] data) =>
        data.Length >= 6 && data[0] == (byte)'G' && data[1] == (byte)'I' && data[2] == (byte)'F';

    /// O avatar salvo ou, sem ele, o padrão. Null só se nem o padrão carregar.
    public static Bitmap? Load()
    {
        try
        {
            if (File.Exists(PngPath))
            {
                using var stream = File.OpenRead(PngPath);
                return new Bitmap(stream);
            }
        }
        catch (Exception)
        {
            // arquivo corrompido: cai no padrão
        }

        return LoadDefault();
    }

    public static Bitmap? LoadDefault()
    {
        try
        {
            using var stream = AssetLoader.Open(DefaultAvatarUri);
            return new Bitmap(stream);
        }
        catch (Exception)
        {
            return null;
        }
    }

    /// Recorta `pixelCrop` (em pixels da imagem) e salva como avatar, substituindo o anterior.
    public static void Save(Bitmap source, Rect pixelCrop)
    {
        // O desenho usa o tamanho "lógico" da imagem, que pode diferir dos pixels (DPI do arquivo)
        var sx = source.Size.Width / source.PixelSize.Width;
        var sy = source.Size.Height / source.PixelSize.Height;
        var sourceRect = new Rect(pixelCrop.X * sx, pixelCrop.Y * sy, pixelCrop.Width * sx, pixelCrop.Height * sy);

        using var target = new RenderTargetBitmap(new PixelSize(Size, Size), new Vector(96, 96));
        using (var context = target.CreateDrawingContext())
        {
            context.DrawImage(source, sourceRect, new Rect(0, 0, Size, Size));
        }

        Directory.CreateDirectory(Folder);

        // Grava num temporário e troca: se falhar no meio, o avatar antigo continua inteiro
        var temp = PngPath + ".tmp";
        target.Save(temp);
        File.Move(temp, PngPath, overwrite: true);

        DeleteOldGifFiles();
    }

    /// Apaga o avatar salvo (o app volta a usar o padrão).
    public static void Remove()
    {
        if (File.Exists(PngPath)) File.Delete(PngPath);
        DeleteOldGifFiles();
    }

    private static void DeleteOldGifFiles()
    {
        if (File.Exists(OldGifPath)) File.Delete(OldGifPath);
        if (File.Exists(OldCropPath)) File.Delete(OldCropPath);
    }
}