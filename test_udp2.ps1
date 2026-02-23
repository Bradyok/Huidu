# Test BOTH port 9526 and 9527 simultaneously to figure out which port the device responds to

Add-Type -TypeDefinition @"
using System;
using System.Net;
using System.Net.Sockets;
using System.Threading;

public class UdpTest {
    public static void Run() {
        // Listen on both ports simultaneously
        UdpClient udp9526 = null;
        UdpClient udp9527 = null;

        try { udp9526 = new UdpClient(9526); Console.WriteLine("Bound to 9526"); }
        catch (Exception e) { Console.WriteLine("Cannot bind 9526: " + e.Message); }

        try { udp9527 = new UdpClient(9527); Console.WriteLine("Bound to 9527"); }
        catch (Exception e) { Console.WriteLine("Cannot bind 9527: " + e.Message); }

        // Send triggers from a separate socket
        UdpClient sender = new UdpClient();
        sender.EnableBroadcast = true;
        byte[] trigOld = new byte[] {2, 0, 1, 0};
        byte[] trigNew = new byte[] {0, 0, 0, 1, 1, 0};

        Console.WriteLine("Sending triggers to 192.168.1.104:9527 and broadcast:9527...");
        sender.Send(trigOld, 4, "192.168.1.104", 9527);
        sender.Send(trigNew, 6, "192.168.1.104", 9527);
        sender.Send(trigOld, 4, "255.255.255.255", 9527);
        // Also try port 9526
        sender.Send(trigOld, 4, "192.168.1.104", 9526);
        sender.Close();
        Console.WriteLine("Triggers sent.");

        // Set timeouts and receive
        if (udp9526 != null) udp9526.Client.ReceiveTimeout = 3000;
        if (udp9527 != null) udp9527.Client.ReceiveTimeout = 3000;

        // Try 9526
        if (udp9526 != null) {
            IPEndPoint ep = new IPEndPoint(IPAddress.Any, 0);
            try {
                byte[] data = udp9526.Receive(ref ep);
                Console.WriteLine("PORT 9526 GOT: " + data.Length + " bytes from " + ep);
                Console.WriteLine(BitConverter.ToString(data));
            } catch { Console.WriteLine("Port 9526: no response in 3s"); }
            udp9526.Close();
        }

        // Try 9527
        if (udp9527 != null) {
            IPEndPoint ep = new IPEndPoint(IPAddress.Any, 0);
            try {
                byte[] data = udp9527.Receive(ref ep);
                Console.WriteLine("PORT 9527 GOT: " + data.Length + " bytes from " + ep);
                Console.WriteLine(BitConverter.ToString(data));
            } catch { Console.WriteLine("Port 9527: no response in 3s"); }
            udp9527.Close();
        }
    }
}
"@
[UdpTest]::Run()
