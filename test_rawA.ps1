# Try with explicit flush + immediate recv after BoxStreamInit
# Also check if device sends InitAck before closing

Add-Type -TypeDefinition @"
using System;
using System.Net;
using System.Net.Sockets;
using System.Threading;

public class TcpTest {
    public static void Run() {
        var tcp = new TcpClient();
        tcp.NoDelay = true;
        tcp.Connect("192.168.1.104", 9527);
        Console.WriteLine("Connected");

        var stream = tcp.GetStream();
        var buf = new byte[1024];

        // Read with timeout helper
        Action<int> setTO = (ms) => { stream.ReadTimeout = ms; };

        // Step 1: Try to read initial packet (device's first packet)
        setTO(3000);
        try {
            int n = stream.Read(buf, 0, 1024);
            var hex = BitConverter.ToString(buf, 0, n);
            Console.WriteLine("Initial recv: " + n + " bytes: " + hex);

            // If TcpHeartbeatAnswer (60 00 in cmd bytes), respond
            if (n >= 4 && buf[2] == 0x60 && buf[3] == 0x00) {
                Console.WriteLine("Got TcpHeartbeatAnswer, sending TcpHeartbeatAsk");
                stream.Write(new byte[]{0x04, 0x00, 0x5F, 0x00}, 0, 4);
                stream.Flush();
                Thread.Sleep(500);
            }
        } catch {
            Console.WriteLine("No initial packet (timeout)");
        }

        // Step 2: Send BoxStreamInit
        Console.WriteLine("Sending BoxStreamInit...");
        var init = new byte[]{0x08, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00};
        stream.Write(init, 0, init.Length);
        stream.Flush();

        // Read all responses
        setTO(500);
        for (int i = 0; i < 5; i++) {
            try {
                int n = stream.Read(buf, 0, 1024);
                if (n == 0) {
                    Console.WriteLine("Connection closed (n=0)");
                    break;
                }
                var hex = BitConverter.ToString(buf, 0, n);
                Console.WriteLine("Recv " + n + " bytes: " + hex);
            } catch (Exception ex) {
                Console.WriteLine("Recv timeout or error: " + ex.Message.Split('\n')[0]);
                break;
            }
        }

        tcp.Close();
    }
}
"@ -Language CSharp

[TcpTest]::Run()
