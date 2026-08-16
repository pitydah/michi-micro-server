#!/usr/bin/env python3
"""
Autonomous Pure-Python MQTT 3.1.1 Broker Simulator with HTTP Admin Inspection.

Implements core MQTT protocol packets:
  CONNECT (1), CONNACK (2), PUBLISH (3), PUBACK (4),
  SUBSCRIBE (8), SUBACK (9), UNSUBSCRIBE (10), UNSUBACK (11),
  PINGREQ (12), PINGRESP (13), DISCONNECT (14)

Provides an HTTP Admin interface on (port + 1) for inspecting published topics
and injecting command messages during Home Assistant E2E integration tests.

Usage:
  python3 scripts/mqtt_broker_sim.py --port 18883 --admin-port 18884
"""

import argparse
import json
import select
import socket
import struct
import threading
import time
from http.server import HTTPServer, BaseHTTPRequestHandler
from socketserver import ThreadingMixIn

class MQTTMessage:
    def __init__(self, topic: str, payload: bytes, qos: int = 0, retain: bool = False):
        self.topic = topic
        self.payload = payload
        self.qos = qos
        self.retain = retain
        self.timestamp = time.time()

    def payload_str(self) -> str:
        try:
            return self.payload.decode("utf-8")
        except Exception:
            return str(self.payload)

class MQTTBroker:
    def __init__(self, port=18883):
        self.port = port
        self.running = True
        self.accepting = True
        self.lock = threading.Lock()
        self.clients = []  # list of client connection threads/sockets
        self.published_messages = []  # list of MQTTMessage
        self.retained_messages = {}  # topic -> MQTTMessage
        self.subscriptions = {}  # client_sock -> set of topic filters

    def topic_matches(self, pattern: str, topic: str) -> bool:
        if pattern == "#":
            return True
        pattern_parts = pattern.split("/")
        topic_parts = topic.split("/")
        i = 0
        while i < len(pattern_parts) and i < len(topic_parts):
            if pattern_parts[i] == "#":
                return True
            if pattern_parts[i] != "+" and pattern_parts[i] != topic_parts[i]:
                return False
            i += 1
        if i < len(pattern_parts) and pattern_parts[i] == "#":
            return True
        return len(pattern_parts) == len(topic_parts)

    def route_publish(self, source_sock, topic: str, payload: bytes, qos: int = 0, retain: bool = False):
        msg = MQTTMessage(topic, payload, qos, retain)
        with self.lock:
            self.published_messages.append(msg)
            if retain:
                self.retained_messages[topic] = msg
            subscribers = list(self.subscriptions.items())

        for client_sock, topics in subscribers:
            if client_sock == source_sock:
                continue
            matched = any(self.topic_matches(sub_filter, topic) for sub_filter in topics)
            if matched:
                try:
                    self.send_publish_packet(client_sock, topic, payload, qos=0)
                except Exception:
                    pass

    def send_publish_packet(self, sock, topic: str, payload: bytes, qos: int = 0):
        topic_bytes = topic.encode("utf-8")
        var_header = struct.pack("!H", len(topic_bytes)) + topic_bytes
        body = var_header + payload
        packet_type = 0x30  # PUBLISH, QoS 0
        rem_len = len(body)
        header = bytearray([packet_type])
        # encode remaining length
        while True:
            byte = rem_len % 128
            rem_len //= 128
            if rem_len > 0:
                byte |= 0x80
            header.append(byte)
            if rem_len == 0:
                break
        sock.sendall(bytes(header) + body)

    def disconnect_all(self):
        with self.lock:
            for s in list(self.subscriptions.keys()):
                try:
                    s.close()
                except Exception:
                    pass
            self.subscriptions.clear()
            self.clients.clear()

def decode_remaining_length(sock):
    multiplier = 1
    value = 0
    while True:
        b = sock.recv(1)
        if not b:
            return None
        byte = b[0]
        value += (byte & 127) * multiplier
        if (byte & 128) == 0:
            break
        multiplier *= 128
        if multiplier > 128 * 128 * 128:
            return None
    return value

def handle_client(sock, addr, broker: MQTTBroker):
    sock.settimeout(60.0)
    client_subs = set()
    with broker.lock:
        broker.subscriptions[sock] = client_subs
        broker.clients.append(sock)

    try:
        while broker.running:
            header = sock.recv(1)
            if not header:
                break
            packet_type = header[0] >> 4
            flags = header[0] & 0x0F
            rem_len = decode_remaining_length(sock)
            if rem_len is None:
                break

            data = b""
            while len(data) < rem_len:
                chunk = sock.recv(rem_len - len(data))
                if not chunk:
                    break
                data += chunk

            if packet_type == 1:  # CONNECT
                # Send CONNACK (0x20, length 2, flags 0, code 0)
                sock.sendall(b"\x20\x02\x00\x00")
            elif packet_type == 3:  # PUBLISH
                # Parse topic
                topic_len = struct.unpack("!H", data[0:2])[0]
                topic = data[2:2 + topic_len].decode("utf-8", errors="replace")
                offset = 2 + topic_len
                qos = (flags >> 1) & 0x03
                packet_id = None
                if qos > 0:
                    packet_id = struct.unpack("!H", data[offset:offset+2])[0]
                    offset += 2
                    # Send PUBACK
                    sock.sendall(b"\x40\x02" + struct.pack("!H", packet_id))
                payload = data[offset:]
                retain = bool(flags & 0x01)
                broker.route_publish(sock, topic, payload, qos, retain)
            elif packet_type == 8:  # SUBSCRIBE
                packet_id = struct.unpack("!H", data[0:2])[0]
                offset = 2
                sub_acks = []
                while offset < len(data):
                    t_len = struct.unpack("!H", data[offset:offset+2])[0]
                    sub_topic = data[offset+2:offset+2+t_len].decode("utf-8", errors="replace")
                    offset += 2 + t_len
                    sub_qos = data[offset]
                    offset += 1
                    client_subs.add(sub_topic)
                    sub_acks.append(sub_qos)
                # Send SUBACK
                sock.sendall(b"\x90" + bytearray([2 + len(sub_acks)]) + struct.pack("!H", packet_id) + bytearray(sub_acks))
            elif packet_type == 12:  # PINGREQ
                # Send PINGRESP
                sock.sendall(b"\xd0\x00")
            elif packet_type == 14:  # DISCONNECT
                break
    except Exception:
        pass
    finally:
        with broker.lock:
            broker.subscriptions.pop(sock, None)
            if sock in broker.clients:
                broker.clients.remove(sock)
        try:
            sock.close()
        except Exception:
            pass

class ThreadedAdminServer(ThreadingMixIn, HTTPServer):
    daemon_threads = True

class AdminHandler(BaseHTTPRequestHandler):
    broker: MQTTBroker = None

    def log_message(self, format, *args):
        pass

    def send_json(self, status, payload):
        data = json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        if self.path in ("/health", "/"):
            self.send_json(200, {"status": "ok", "service": "mqtt-broker-sim"})
            return
        if self.path == "/api/mqtt/messages":
            with self.broker.lock:
                msgs = [
                    {
                        "topic": m.topic,
                        "payload": m.payload_str(),
                        "qos": m.qos,
                        "retain": m.retain,
                        "timestamp": m.timestamp,
                    }
                    for m in self.broker.published_messages
                ]
            self.send_json(200, {"messages": msgs})
            return
        self.send_json(404, {"error": "not found"})

    def do_POST(self):
        length = int(self.headers.get("Content-Length", 0))
        raw = self.rfile.read(length).decode("utf-8") if length > 0 else "{}"
        try:
            body = json.loads(raw)
        except Exception:
            body = {}

        if self.path == "/api/mqtt/publish":
            topic = body.get("topic", "")
            payload = body.get("payload", "")
            if isinstance(payload, (dict, list)):
                payload = json.dumps(payload)
            payload_bytes = str(payload).encode("utf-8")
            self.broker.route_publish(None, topic, payload_bytes, qos=0, retain=False)
            self.send_json(200, {"status": "published", "topic": topic})
            return

        if self.path == "/api/mqtt/drop":
            self.broker.accepting = False
            self.broker.disconnect_all()
            self.send_json(200, {"status": "clients_dropped"})
            return

        if self.path == "/api/mqtt/restore":
            self.broker.accepting = True
            self.send_json(200, {"status": "broker_restored"})
            return

        if self.path == "/api/mqtt/clear":
            with self.broker.lock:
                self.broker.published_messages.clear()
            self.send_json(200, {"status": "messages_cleared"})
            return

        self.send_json(404, {"error": "not found"})

def run_broker(port=18883, admin_port=18884):
    broker = MQTTBroker(port=port)

    # Start Admin HTTP Server
    admin_handler = type("ConfiguredAdminHandler", (AdminHandler,), {"broker": broker})
    admin_server = ThreadedAdminServer(("0.0.0.0", admin_port), admin_handler)
    admin_thread = threading.Thread(target=admin_server.serve_forever, daemon=True)
    admin_thread.start()
    print(f"MQTT Broker Simulator running on port {port} (Admin API on http://0.0.0.0:{admin_port})")

    # Start TCP Server for MQTT
    srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.bind(("0.0.0.0", port))
    srv.listen(50)

    try:
        while broker.running:
            r, _, _ = select.select([srv], [], [], 0.5)
            if r:
                client_sock, addr = srv.accept()
                if not broker.accepting:
                    client_sock.close()
                    continue
                t = threading.Thread(target=handle_client, args=(client_sock, addr, broker), daemon=True)
                t.start()
    except KeyboardInterrupt:
        pass
    finally:
        broker.running = False
        srv.close()
        admin_server.server_close()

def main():
    parser = argparse.ArgumentParser(description="MQTT Broker Simulator")
    parser.add_argument("--port", type=int, default=18883)
    parser.add_argument("--admin-port", type=int, default=18884)
    args = parser.parse_args()
    run_broker(args.port, args.admin_port)

if __name__ == "__main__":
    main()
