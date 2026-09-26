import av

import cv2

import av

# Ordem de tentativa: GPU Nvidia -> GPU AMD -> GPU Intel -> software (CPU) como último recurso
ENCODER_CANDIDATES = ["h264_nvenc", "h264_amf", "h264_qsv", "libx264"]

ENCODER_LABELS = {
    "h264_nvenc": "GPU (Nvidia NVENC)",
    "h264_amf": "GPU (AMD AMF)",
    "h264_qsv": "GPU (Intel Quick Sync)",
    "libx264": "Software (CPU)",
}


def create_encoder(width, height, fps, bitrate_kbps):
    """Tenta criar um encoder H.264 acelerado por GPU, nessa ordem: Nvidia, AMD,
    Intel. Se nenhuma dessas estiver disponível (ou a inicialização falhar por
    qualquer motivo), cai para o encoder por software (libx264), que funciona
    em qualquer computador. Retorna (codec_context, nome_usado, rótulo_amigável)."""
    last_error = None
    for name in ENCODER_CANDIDATES:
        try:
            ctx = av.CodecContext.create(name, "w")
            ctx.width = width
            ctx.height = height
            ctx.pix_fmt = "yuv420p"
            ctx.bit_rate = int(bitrate_kbps * 1000)
            ctx.gop_size = max(1, int(fps) * 2)  # keyframe a cada ~2 segundos

            if name == "libx264":
                ctx.options = {"preset": "ultrafast", "tune": "zerolatency"}
            elif name == "h264_nvenc":
                ctx.options = {"preset": "p1", "tune": "ll", "rc": "cbr"}
            elif name == "h264_amf":
                ctx.options = {"usage": "ultralowlatency", "rc": "cbr"}
            elif name == "h264_qsv":
                ctx.options = {"preset": "veryfast"}

            ctx.open()
            return ctx, name, ENCODER_LABELS[name]
        except Exception as e:
            last_error = e
            continue

    raise RuntimeError(f"Nenhum encoder H.264 disponível neste computador (última tentativa: {last_error})")


def create_decoder():
    """Cria um decoder H.264 por software — decodificar é bem mais leve que
    codificar, então funciona tranquilo em qualquer hardware, independente
    de qual GPU o Host usou para gerar o vídeo."""
    return av.CodecContext.create("h264", "r")


def encode_frame(codec_context, bgr_frame):
    """Recebe um frame cru (array numpy BGR) e devolve uma lista de pacotes
    H.264 codificados. A conversão BGR -> YUV420p é feita via OpenCV (rápido,
    otimizado por SIMD) em vez do reformat() interno do PyAV (que usa
    libswscale por software e era o principal gargalo de performance)."""
    yuv_frame = cv2.cvtColor(bgr_frame, cv2.COLOR_BGR2YUV_I420)
    frame = av.VideoFrame.from_ndarray(yuv_frame, format="yuv420p")
    return codec_context.encode(frame)


def decode_packet(codec_context, packet_bytes):
    """Recebe os bytes de um pacote H.264 recebido pela rede e devolve uma
    lista de frames decodificados (arrays numpy BGR)."""
    packet = av.Packet(packet_bytes)
    frames = codec_context.decode(packet)
    return [f.to_ndarray(format="bgr24") for f in frames]