# Raw byte dump - capture EVERYTHING the device sends for 10 seconds
# Also test different SDK versions

Add-Type -TypeDefinition @"
using System;
using System.Net.Sockets;
using System.Text;
using System.IO;
using System.Collections.Generic;

public class RawTest {
    static byte[] MakePkt(ushort cmd, byte[] payload) {
        // TOTAL format: length = 4 + payload.len
        int total = 4 + payload.Length;
        byte[] pkt = new byte[total];
        pkt[0] = (byte)(total & 0xFF);
        pkt[1] = (byte)((total >> 8) & 0xFF);
        pkt[2] = (byte)(cmd & 0xFF);
        pkt[3] = (byte)((cmd >> 8) & 0xFF);
        Array.Copy(payload, 0, pkt, 4, payload.Length);
        return pkt;
    }

    static List<byte[]> ReadAllBytes(NetworkStream ns, int durationMs) {
        var result = new List<byte[]>();
        var buf = new byte[4096];
        ns.ReadTimeout = 500;
        var deadline = DateTime.Now.AddMilliseconds(durationMs);
        var acc = new List<byte>();
        while (DateTime.Now < deadline) {
            try {
                int n = ns.Read(buf, 0, buf.Length);
                if (n == 0) break;
                for (int i = 0; i < n; i++) acc.Add(buf[i]);
            } catch (IOException) { }
        }
        if (acc.Count > 0) result.Add(acc.ToArray());
        return result;
    }

    public static void Test(string host, uint version) {
        Console.WriteLine("");
        Console.WriteLine("=== SDK version 0x" + version.ToString("X8") + " ===");
        try {
            var tcp = new TcpClient();
            tcp.Connect(host, 10001);
            var ns = tcp.GetStream();
            Console.WriteLine("Connected");

            // Send SdkServiceAsk
            var vBytes = new byte[4];
            vBytes[0] = (byte)(version & 0xFF);
            vBytes[1] = (byte)((version >> 8) & 0xFF);
            vBytes[2] = (byte)((version >> 16) & 0xFF);
            vBytes[3] = (byte)((version >> 24) & 0xFF);
            var svcPkt = MakePkt(0x2001, vBytes);
            Console.WriteLine("Send SdkServiceAsk: " + BitConverter.ToString(svcPkt));
            ns.Write(svcPkt, 0, svcPkt.Length);

            // Read raw for 3 seconds
            var resp1 = ReadAllBytes(ns, 3000);
            if (resp1.Count > 0 && resp1[0].Length > 0)
                Console.WriteLine("SdkServiceAsk response (" + resp1[0].Length + " bytes): " + BitConverter.ToString(resp1[0]));
            else
                Console.WriteLine("No response to SdkServiceAsk");

            // Send GetDeviceInfo
            string guid = "AAAAAAAA-AAAA-AAAA-AAAA-AAAAAAAAAAAA";
            string xml = "<?xml version='1.0' encoding='utf-8'?><sdk guid=\"" + guid + "\"><in method=\"GetDeviceInfo\"></in></sdk>";
            byte[] xmlBytes = Encoding.UTF8.GetBytes(xml);
            byte[] framing = new byte[8 + xmlBytes.Length];
            framing[0] = (byte)(xmlBytes.Length & 0xFF);
            framing[1] = (byte)((xmlBytes.Length >> 8) & 0xFF);
            framing[2] = 0; framing[3] = 0;
            framing[4] = 0; framing[5] = 0; framing[6] = 0; framing[7] = 0;
            Array.Copy(xmlBytes, 0, framing, 8, xmlBytes.Length);
            var cmdPkt = MakePkt(0x2003, framing);
            Console.WriteLine("Send GetDeviceInfo (" + cmdPkt.Length + " bytes)");
            ns.Write(cmdPkt, 0, cmdPkt.Length);

            // Read raw for 8 seconds
            var resp2 = ReadAllBytes(ns, 8000);
            if (resp2.Count > 0 && resp2[0].Length > 0) {
                byte[] data = resp2[0];
                Console.WriteLine("GetDeviceInfo response (" + data.Length + " bytes):");
                Console.WriteLine("  Hex: " + BitConverter.ToString(data));
                // Try to decode as string
                if (data.Length > 4) {
                    try {
                        string s = Encoding.UTF8.GetString(data, 4, data.Length - 4);
                        Console.WriteLine("  AsStr[4..]: " + s.Substring(0, Math.Min(s.Length, 300)));
                    } catch {}
                }
            } else {
                Console.WriteLine("No response to GetDeviceInfo");
            }

            tcp.Close();
        } catch (Exception ex) {
            Console.WriteLine("ERROR: " + ex.Message);
        }
    }
}
"@

# Test different versions
[RawTest]::Test("192.168.1.104", 0x01000000)
Start-Sleep -Milliseconds 500
[RawTest]::Test("192.168.1.104", 0x07000000)
Start-Sleep -Milliseconds 500
[RawTest]::Test("192.168.1.104", 0x00010000)
Start-Sleep -Milliseconds 500
# Also test with NO SdkServiceAsk - just send SdkCmdAsk directly
Add-Type -TypeDefinition @"
using System;
using System.Net.Sockets;
using System.Text;
using System.IO;
using System.Collections.Generic;

public class RawTest2 {
    static byte[] MakePkt(ushort cmd, byte[] payload) {
        int total = 4 + payload.Length;
        byte[] pkt = new byte[total];
        pkt[0] = (byte)(total & 0xFF);
        pkt[1] = (byte)((total >> 8) & 0xFF);
        pkt[2] = (byte)(cmd & 0xFF);
        pkt[3] = (byte)((cmd >> 8) & 0xFF);
        Array.Copy(payload, 0, pkt, 4, payload.Length);
        return pkt;
    }

    static byte[] ReadRaw(NetworkStream ns, int ms) {
        var acc = new System.Collections.Generic.List<byte>();
        var buf = new byte[4096];
        ns.ReadTimeout = 500;
        var dl = DateTime.Now.AddMilliseconds(ms);
        while (DateTime.Now < dl) {
            try {
                int n = ns.Read(buf, 0, buf.Length);
                if (n == 0) break;
                for (int i = 0; i < n; i++) acc.Add(buf[i]);
            } catch (IOException) {}
        }
        return acc.ToArray();
    }

    public static void Test(string host) {
        Console.WriteLine("");
        Console.WriteLine("=== No handshake - direct SdkCmdAsk ===");
        try {
            var tcp = new TcpClient();
            tcp.Connect(host, 10001);
            var ns = tcp.GetStream();
            Console.WriteLine("Connected");
            string guid = "BBBBBBBB-BBBB-BBBB-BBBB-BBBBBBBBBBBB";
            string xml = "<?xml version='1.0' encoding='utf-8'?><sdk guid=\"" + guid + "\"><in method=\"GetDeviceInfo\"></in></sdk>";
            byte[] xmlBytes = Encoding.UTF8.GetBytes(xml);
            byte[] framing = new byte[8 + xmlBytes.Length];
            framing[0] = (byte)(xmlBytes.Length & 0xFF); framing[1] = (byte)((xmlBytes.Length >> 8) & 0xFF);
            Array.Copy(xmlBytes, 0, framing, 8, xmlBytes.Length);
            var pkt = MakePkt(0x2003, framing);
            Console.WriteLine("Send SdkCmdAsk directly (" + pkt.Length + " bytes)");
            ns.Write(pkt, 0, pkt.Length);
            byte[] resp = ReadRaw(ns, 8000);
            if (resp.Length > 0) {
                Console.WriteLine("Response (" + resp.Length + " bytes): " + BitConverter.ToString(resp));
                if (resp.Length > 4) {
                    try { Console.WriteLine("AsStr[4..]: " + Encoding.UTF8.GetString(resp, 4, resp.Length-4).Substring(0, Math.Min(resp.Length-4, 300))); } catch {}
                }
            } else Console.WriteLine("No response");
            tcp.Close();
        } catch (Exception ex) { Console.WriteLine("ERROR: " + ex.Message); }
    }
}
"@

[RawTest2]::Test("192.168.1.104")
