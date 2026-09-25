import socket

HOST = "0.0.0.0"   # aceita conexões de qualquer IP na rede local
PORT = 5555        # porta que o Client vai usar para conectar

def start_host():
    server_socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    server_socket.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    server_socket.bind((HOST, PORT))
    server_socket.listen(1)  # por enquanto, só 1 Client por vez

    print(f"[HOST] Aguardando conexão na porta {PORT}...", flush=True)

    conn, addr = server_socket.accept()
    print(f"[HOST] Cliente conectado: {addr}")

    try:
        conn.sendall("Conectado ao Host com sucesso!".encode("utf-8"))

        while True:
            data = conn.recv(1024)
            if not data:
                print("[HOST] Cliente desconectou.")
                break
            print(f"[HOST] Mensagem recebida: {data.decode('utf-8')}")
    except ConnectionResetError:
        print("[HOST] Conexão perdida com o cliente.")
    finally:
        conn.close()
        server_socket.close()
        print("[HOST] Servidor encerrado.")

if __name__ == "__main__":
    start_host()