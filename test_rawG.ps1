# Add error checking to distinguish RST vs FIN vs InitAck+close
Add-Type -TypeDefinition @"
using System;
using System.Net;
using System.Net.Sockets;
using System.Threading;

public class FullTest {
    public static void Run() {
        var tcp = new TcpClient();
        tcp.NoDelay = true;
        tcp.Connect("192.168.1.104", 9527);
        Console.WriteLine("Connected at " + DateTime.Now.ToString("HH:mm:ss.fff"));

        var ns = tcp.GetStream();
        var buf = new byte[1024];

        // Wait up to 12 seconds for device's TcpHeartbeatAnswer
        ns.ReadTimeout = 12000;
        try {
            int n = ns.Read(buf, 0, 1024);
            if (n > 0) {
                Console.WriteLine("Initial recv " + n + " bytes: " + BitConverter.ToString(buf, 0, n));
                ushort cmd = (ushort)(buf[2] | (buf[3] << 8));
                Console.WriteLine("Cmd: 0x" + cmd.ToString("X4"));

                if (cmd == 0x0060) {
                    // TcpHeartbeatAnswer - respond
                    Console.WriteLine("Responding with TcpHeartbeatAsk");
                    ns.Write(new byte[]{0x04, 0x00, 0x5F, 0x00}, 0, 4);
                    ns.Flush();
                    Thread.Sleep(200);
                }
            }
        } catch (Exception ex) {
            Console.WriteLine("Initial recv error: " + ex.Message.Split('\n')[0]);
        }

        // Send BoxStreamInit
        Console.WriteLine("Sending BoxStreamInit...");
        ns.Write(new byte[]{0x08, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00}, 0, 8);
        ns.Flush();
        Console.WriteLine("Sent BoxStreamInit at " + DateTime.Now.ToString("HH:mm:ss.fff"));

        // Read response with small 500ms timeout to catch quick response
        ns.ReadTimeout = 500;
        for (int i = 0; i < 20; i++) {
            try {
                int n = ns.Read(buf, 0, 1024);
                if (n == 0) {
                    Console.WriteLine("n=0 (FIN or RST received)");
                    break;
                }
                Console.WriteLine("Recv " + n + " bytes: " + BitConverter.ToString(buf, 0, n));
                ushort cmd = (ushort)(buf[2] | (buf[3] << 8));
                Console.WriteLine("Cmd: 0x" + cmd.ToString("X4"));
                if (cmd == 0x0201) {
                    Console.WriteLine("BoxStreamInitAck received!");
                    break;
                }
            } catch (IOException ioex) {
                Console.WriteLine("IO exception: " + ioex.Message.Split('\n')[0]);
                break;
            } catch (Exception ex) {
                Console.WriteLine("Read timeout (500ms): " + ex.Message.Substring(0, Math.Min(50, ex.Message.Length)));
                break;
            }
        }

        tcp.Close();
    }
}
"@ -Language CSharp

[FullTest]::Run()
