# Correct protocol test:
# - PC should listen on port 9527 (device sends FROM 9526 TO 9527 on PC)
# - PC sends trigger from port 9527 to device:9527 AND broadcast:9527
# - When PC receives 21-byte announce FROM device port 9526, echo to device:9527
# - Then receive full response

Add-Type -TypeDefinition @"
using System;
using System.Net;
using System.Net.Sockets;
using System.Threading;
using System.Collections.Generic;

public class UdpTest2 {
    public static void Run() {
        // Bind to port 9527 so our SOURCE port is 9527 when sending
        UdpClient sock = null;
        try {
            sock = new UdpClient(9527);
            sock.EnableBroadcast = true;
            Console.WriteLine("Bound to port 9527 OK");
        } catch (Exception e) {
            Console.WriteLine("Cannot bind 9527: " + e.Message);
            // Fall back to random port
            sock = new UdpClient();
            sock.EnableBroadcast = true;
            Console.WriteLine("Using random source port instead");
        }

        // Send triggers - old format, new format, and raw bytes
        byte[] trigOld = new byte[] {2, 0, 1, 0};
        byte[] trigNew = new byte[] {0, 0, 0, 1, 1, 0};

        Console.WriteLine("Sending triggers...");
        try { sock.Send(trigOld, trigOld.Length, "192.168.1.104", 9527); } catch {}
        try { sock.Send(trigNew, trigNew.Length, "192.168.1.104", 9527); } catch {}
        try { sock.Send(trigOld, trigOld.Length, "255.255.255.255", 9527); } catch {}
        // Also try sending a raw announce packet as trigger
        byte[] rawEcho = new byte[] {0, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0};
        try { sock.Send(rawEcho, rawEcho.Length, "192.168.1.104", 9527); } catch {}
        Console.WriteLine("Triggers sent. Listening on port 9527 for 8 seconds...");

        sock.Client.ReceiveTimeout = 8000;
        IPEndPoint ep = new IPEndPoint(IPAddress.Any, 0);
        DateTime deadline = DateTime.Now.AddSeconds(8);
        int packetCount = 0;

        while (DateTime.Now < deadline) {
            sock.Client.ReceiveTimeout = (int)(deadline - DateTime.Now).TotalMilliseconds;
            if (sock.Client.ReceiveTimeout <= 0) break;
            try {
                byte[] data = sock.Receive(ref ep);
                packetCount++;
                Console.WriteLine("Packet #" + packetCount + ": " + data.Length + " bytes FROM " + ep);
                Console.WriteLine("  Hex: " + BitConverter.ToString(data));

                // If it's our own echo, skip
                if (ep.Address.ToString() == "192.168.1.100" || ep.Address.ToString() == "127.0.0.1") {
                    Console.WriteLine("  (own loopback, skipping)");
                    continue;
                }

                // If 21-byte new-format announce (from device)
                if (data.Length == 21 && data.Length >= 6 && data[3] == 0x01 && data[4] == 0x02) {
                    Console.WriteLine("  -> SHORT ANNOUNCE! Echoing to " + ep.Address + ":9527");
                    sock.Send(data, data.Length, ep.Address.ToString(), 9527);
                    Console.WriteLine("  Echo sent, waiting for full response...");
                }
            } catch (SocketException) { break; }
        }

        Console.WriteLine("Done. Total packets: " + packetCount);
        sock.Close();
    }
}
"@
[UdpTest2]::Run()
