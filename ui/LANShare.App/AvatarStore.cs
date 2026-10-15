using System.Runtime.InteropServices;
using System.Text.Json;
using Avalonia;
using Avalonia.Media.Imaging;
using Avalonia.Platform;
using SkiaSharp;

namespace LANShare.App;

/// Um avatar pronto para mostrar: 1 quadro (imagem comum) ou vários (GIF animado).
internal sealed class AvatarImage
{
    public required IReadOnlyList<Bitmap> Frames { get; init; }
    public required IReadOnlyList<TimeSpan> Delays { get; init; }

    public bool IsAnimated => Frames.Count > 1;

    public static AvatarImage Still(Bitmap bitmap) =>
        new() { Frames = new[] { bitmap }, Delays = new[] { TimeSpan.Zero } };
}

/// Avatar do usuário, em %APPDATA%\LANShare:
/// imagem comum -> avatar.png (já recortada);
/// GIF -> avatar.gif (original) + avatar_crop.json (onde fica o círculo).
/// Sem nada salvo, usa o avatar padrão que vem dentro do app.
internal static class AvatarStore
{
    /// Lado do avatar salvo como PNG, em pixels.
    public const int Size = 256;

    /// Lado de cada quadro do GIF depois do recorte (o avatar aparece pequeno).
    private const int GifFrameSize = 128;

    /// Limites para um GIF não pesar demais.
    public const long MaxGifBytes = 15 * 1024 * 1024;
    private const int MaxGifFrames = 300;

    private static readonly Uri DefaultAvatarUri = new("avares://LANShare/Assets/default_avatar.jpg");

    private sealed record CropData(double X, double Y, double Side);

    private static string Folder =>
        Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "LANShare");

    private static string PngPath => Path.Combine(Folder, "avatar.png");
    private static string GifPath => Path.Combine(Folder, "avatar.gif");
    private static string CropPath => Path.Combine(Folder, "avatar_crop.json");

    /// Existe um avatar escolhido pelo usuário?
    public static bool HasCustom => File.Exists(PngPath) || File.Exists(GifPath);

    /// Os bytes são de um GIF? (começa com "GIF")
    public static bool IsGif(byte[] data) =>
        data.Length >= 6 && data[0] == (byte)'G' && data[1] == (byte)'I' && data[2] == (byte)'F';

    /// O avatar salvo ou, sem ele, o padrão. Null só se nem o padrão carregar.
    /// Um GIF leva uma fração de segundo para preparar: chame fora da thread da interface.
    public static AvatarImage? Load()
    {
        try
        {
            if (File.Exists(GifPath) && File.Exists(CropPath))
            {
                var crop = JsonSerializer.Deserialize<CropData>(File.ReadAllText(CropPath));
                if (crop is not null)
                {
                    var gif = DecodeGif(File.ReadAllBytes(GifPath), new Rect(crop.X, crop.Y, crop.Side, crop.Side));
                    if (gif is not null) return gif;
                }
            }

            if (File.Exists(PngPath))
            {
                using var stream = File.OpenRead(PngPath);
                return AvatarImage.Still(new Bitmap(stream));
            }
        }
        catch (Exception)
        {
            // arquivo corrompido: cai no padrão
        }

        return LoadDefault();
    }

    public static AvatarImage? LoadDefault()
    {
        try
        {
            using var stream = AssetLoader.Open(DefaultAvatarUri);
            return AvatarImage.Still(new Bitmap(stream));
        }
        catch (Exception)
        {
            return null;
        }
    }

    /// Recorta `pixelCrop` (em pixels da imagem) e salva como avatar.png, substituindo o anterior.
    public static void SaveStill(Bitmap source, Rect pixelCrop)
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

        DeleteIfExists(GifPath);
        DeleteIfExists(CropPath);
    }

    /// Salva o GIF original e a posição do recorte, substituindo o avatar anterior.
    public static void SaveGif(byte[] gif, Rect pixelCrop)
    {
        // Confere antes que o GIF abre, para não trocar um avatar bom por um quebrado
        if (DecodeGif(gif, pixelCrop) is null)
            throw new InvalidOperationException("não foi possível ler os quadros desse GIF");

        Directory.CreateDirectory(Folder);

        var gifTemp = GifPath + ".tmp";
        var cropTemp = CropPath + ".tmp";
        File.WriteAllBytes(gifTemp, gif);
        File.WriteAllText(cropTemp, JsonSerializer.Serialize(new CropData(pixelCrop.X, pixelCrop.Y, pixelCrop.Width)));
        File.Move(gifTemp, GifPath, overwrite: true);
        File.Move(cropTemp, CropPath, overwrite: true);

        DeleteIfExists(PngPath);
    }

    /// Apaga o avatar salvo, de qualquer tipo (o app volta a usar o padrão).
    public static void Remove()
    {
        DeleteIfExists(PngPath);
        DeleteIfExists(GifPath);
        DeleteIfExists(CropPath);
    }

    private static void DeleteIfExists(string path)
    {
        if (File.Exists(path)) File.Delete(path);
    }

    // ───────────────────────── GIF ─────────────────────────

    /// Lê os quadros do GIF e recorta cada um no círculo escolhido.
    private static AvatarImage? DecodeGif(byte[] data, Rect crop)
    {
        using var stream = new SKMemoryStream(data);
        using var codec = SKCodec.Create(stream);
        if (codec is null) return null;

        var info = new SKImageInfo(codec.Info.Width, codec.Info.Height, SKColorType.Bgra8888, SKAlphaType.Premul);
        var frameInfos = codec.FrameInfo;
        var count = Math.Clamp(codec.FrameCount, 1, MaxGifFrames);

        var frames = new List<Bitmap>(count);
        var delays = new List<TimeSpan>(count);

        using var full = new SKBitmap(info);
        for (var i = 0; i < count; i++)
        {
            // Sem "quadro anterior" informado, o Skia monta sozinho os quadros dos quais este depende
            var result = codec.GetPixels(info, full.GetPixels(), new SKCodecOptions(i));
            if (result != SKCodecResult.Success && result != SKCodecResult.IncompleteInput) break;

            frames.Add(CropFrame(full, crop));

            // Navegadores tratam atrasos muito curtos como 100 ms; fazemos igual
            var ms = i < frameInfos.Length ? frameInfos[i].Duration : 100;
            delays.Add(TimeSpan.FromMilliseconds(ms < 20 ? 100 : ms));
        }

        return frames.Count == 0 ? null : new AvatarImage { Frames = frames, Delays = delays };
    }

    /// Recorta o quadrado do círculo e reduz para GifFrameSize (média dos pixels cobertos).
    private static Bitmap CropFrame(SKBitmap full, Rect crop)
    {
        const int size = GifFrameSize;
        var (w, h) = (full.Width, full.Height);
        var src = full.Bytes;
        var stride = full.RowBytes;
        var dst = new byte[size * size * 4];

        for (var dy = 0; dy < size; dy++)
        {
            var y0 = Math.Clamp((int)(crop.Y + dy * crop.Height / size), 0, h - 1);
            var y1 = Math.Clamp((int)Math.Ceiling(crop.Y + (dy + 1) * crop.Height / size), y0 + 1, h);

            for (var dx = 0; dx < size; dx++)
            {
                var x0 = Math.Clamp((int)(crop.X + dx * crop.Width / size), 0, w - 1);
                var x1 = Math.Clamp((int)Math.Ceiling(crop.X + (dx + 1) * crop.Width / size), x0 + 1, w);

                int b = 0, g = 0, r = 0, a = 0;
                for (var y = y0; y < y1; y++)
                {
                    var row = y * stride;
                    for (var x = x0; x < x1; x++)
                    {
                        var p = row + x * 4;
                        b += src[p];
                        g += src[p + 1];
                        r += src[p + 2];
                        a += src[p + 3];
                    }
                }

                var n = (y1 - y0) * (x1 - x0);
                var o = (dy * size + dx) * 4;
                dst[o] = (byte)(b / n);
                dst[o + 1] = (byte)(g / n);
                dst[o + 2] = (byte)(r / n);
                dst[o + 3] = (byte)(a / n);
            }
        }

        var bitmap = new WriteableBitmap(
            new PixelSize(size, size), new Vector(96, 96), PixelFormat.Bgra8888, AlphaFormat.Premul);
        using (var frame = bitmap.Lock())
        {
            for (var y = 0; y < size; y++)
                Marshal.Copy(dst, y * size * 4, frame.Address + y * frame.RowBytes, size * 4);
        }
        return bitmap;
    }
}