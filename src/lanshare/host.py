import socket
import threading

HOST = "0.0.0.0"
PORT = 5555

connected = True


def receive_messages(conn):
    global connected
    while connected:
        try:
            data = conn.recv(1024)
            if not data:
                print("\n[HOST] Cliente desconectou.")
                connected = False
                break
            print(f"\n[HOST] Mensagem recebida: {data.decode('utf-8')}")
        except (ConnectionResetError, OSError):
            print("\n[HOST] Conexão perdida com o cliente.")
            connected = False
            break


def start_host():
    global connected
    server_socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    server_socket.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    server_socket.bind((HOST, PORT))
    server_socket.listen(1)

    print(f"[HOST] Aguardando conexão na porta {PORT}...", flush=True)

    conn, addr = server_socket.accept()
    print(f"[HOST] Cliente conectado: {addr}")

    conn.sendall("Conectado ao Host com sucesso!".encode("utf-8"))

    receiver_thread = threading.Thread(target=receive_messages, args=(conn,), daemon=True)
    receiver_thread.start()

    try:
        while connected:
            msg = input()
            if not connected:
                break
            conn.sendall(msg.encode("utf-8"))
    except (ConnectionResetError, OSError):
        pass
    finally:
        connected = False
        conn.close()
        server_socket.close()
        print("[HOST] Servidor encerrado.")


if __name__ == "__main__":
    start_host()