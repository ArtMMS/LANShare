import socket
import threading

PORT = 5555  # tem que ser a mesma porta usada no host.py

connected = True  # controla se ainda estamos conectados


def receive_messages(sock):
    global connected
    while connected:
        try:
            data = sock.recv(1024)
            if not data:
                print("\n[CLIENT] O Host desconectou.")
                connected = False
                break
            print(f"\n[CLIENT] Mensagem do Host: {data.decode('utf-8')}")
        except (ConnectionResetError, OSError):
            print("\n[CLIENT] Conexão perdida com o Host.")
            connected = False
            break


def start_client():
    global connected
    host_ip = input("Digite o IP do Host: ").strip()

    client_socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)

    try:
        client_socket.connect((host_ip, PORT))
        print(f"[CLIENT] Conectado ao Host {host_ip}:{PORT}", flush=True)
    except ConnectionRefusedError:
        print("[CLIENT] Não foi possível conectar. O Host está rodando e o IP está correto?")
        return

    # thread separada só para ficar "escutando" o Host o tempo todo
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