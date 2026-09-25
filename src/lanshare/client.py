import socket
import threading

PORT = 5555

connected = True
host_username = "Host"  # nome padrão até recebermos o real


def receive_messages(sock):
    global connected, host_username
    while connected:
        try:
            data = sock.recv(1024)
            if not data:
                print("\n[CLIENT] O Host desconectou.")
                connected = False
                break
            print(f"\n{host_username}: {data.decode('utf-8')}")
        except (ConnectionResetError, OSError):
            print("\n[CLIENT] Conexão perdida com o Host.")
            connected = False
            break


def start_client():
    global connected, host_username

    host_ip = input("Digite o IP do Host: ").strip()
    username = input("Digite seu nome de usuário: ").strip() or "Client"

    client_socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)

    try:
        client_socket.connect((host_ip, PORT))
        print(f"[CLIENT] Conectado ao Host {host_ip}:{PORT}", flush=True)
    except ConnectionRefusedError:
        print("[CLIENT] Não foi possível conectar. O Host está rodando e o IP está correto?")
        return

    # troca de nomes de usuário logo após conectar
    client_socket.sendall(username.encode("utf-8"))
    host_username = client_socket.recv(1024).decode("utf-8")
    print(f"[CLIENT] Conectado com: {host_username}")

    receiver_thread = threading.Thread(target=receive_messages, args=(client_socket,), daemon=True)
    receiver_thread.start()

    try:
        while connected:
            msg = input()
            if not connected:
                break
            if msg.lower() == "sair":
                break
            client_socket.sendall(msg.encode("utf-8"))
    except (ConnectionResetError, OSError):
        pass
    finally:
        connected = False
        client_socket.close()
        print("[CLIENT] Desconectado.")


if __name__ == "__main__":
    start_client()