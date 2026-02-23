# Test different SdkCmdAsk framing variants
# Using confirmed correct format: length = total packet size

Add-Type -TypeDefinition @"
using System;
using System.Net.Sockets;
using System.Text;
using System.IO;

public class TcpTest {
    static byte[] MakePacket(ushort cmd, byte[] payload) {
        int total = 4 + payload.Length;
        byte[] pkt = new byte[total];
        pkt[0] = (byte)(total & 0xFF);
        pkt[1] = (byte)((total >> 8) & 0xFF);
        pkt[2] = (byte)(cmd & 0xFF);
        pkt[3] = (byte)((cmd >> 8) & 0xFF);
        Array.Copy(payload, 0, pkt, 4, payload.Length);
        return pkt;
    }

    static byte[] Recv(NetworkStream ns, int timeoutMs) {
        ns.ReadTimeout = timeoutMs;
        byte[] header = new byte[4];
        int got = 0;
        while (got < 4) {
            int n = ns.Read(header, got, 4 - got);
            if (n == 0) return null;
            got += n;
        }
        int totalLen = header[0] | (header[1] << 8);
        ushort cmd = (ushort)(header[2] | (header[3] << 8));
        int payloadLen = totalLen - 4;
        Console.WriteLine("  RECV: total=" + totalLen + " cmd=0x" + cmd.ToString("X4") + " payloadLen=" + payloadLen);
        byte[] payload = new byte[payloadLen];
        got = 0;
        while (got < payloadLen) {
            int n = ns.Read(payload, got, payloadLen - got);
            if (n == 0) break;
            got += n;
        }
        if (payloadLen > 0) {
            Console.WriteLine("  PAYLOAD[" + got + "]: " + BitConverter.ToString(payload, 0, Math.Min(got, 64)));
            if (got > 8) {
                // Try to print as string
                try {
                    string s = Encoding.UTF8.GetString(payload, 8, Math.Min(got - 8, 200));
                    Console.WriteLine("  AS_STR: " + s.Substring(0, Math.Min(s.Length, 200)));
                } catch {}
            }
        }
        return payload;
    }

    public static void Run(string host, string variant) {
        Console.WriteLine("=== Testing variant: " + variant + " ===");
        TcpClient tcp = new TcpClient();
        tcp.Connect(host, 10001);
        NetworkStream ns = tcp.GetStream();
        Console.WriteLine("Connected");

        // Step 1: SdkServiceAsk (cmd=0x2001, payload=[00 00 00 01])
        byte[] svcPayload = new byte[] { 0x00, 0x00, 0x00, 0x01 };
        byte[] svcPkt = MakePacket(0x2001, svcPayload);
        Console.WriteLine("SEND SdkServiceAsk: " + BitConverter.ToString(svcPkt));
        ns.Write(svcPkt, 0, svcPkt.Length);
        Recv(ns, 5000);

        // Step 2: SdkCmdAsk for GetDeviceInfo with different framing
        string guid = "11111111-1111-1111-1111-111111111111";
        string xml = "<?xml version='1.0' encoding='utf-8'?><sdk guid=\"" + guid + "\"><in method=\"GetDeviceInfo\"></in></sdk>";
        byte[] xmlBytes = Encoding.UTF8.GetBytes(xml);
        Console.WriteLine("XML (" + xmlBytes.Length + " bytes): " + xml.Substring(0, Math.Min(xml.Length, 80)));

        byte[] cmdPayload;
        if (variant == "raw") {
            // No framing - just raw XML
            cmdPayload = xmlBytes;
        } else if (variant == "4byte") {
            // 4-byte framing: [u32 total_xml_len][xml]
            cmdPayload = new byte[4 + xmlBytes.Length];
            cmdPayload[0] = (byte)(xmlBytes.Length & 0xFF);
            cmdPayload[1] = (byte)((xmlBytes.Length >> 8) & 0xFF);
            cmdPayload[2] = 0; cmdPayload[3] = 0;
            Array.Copy(xmlBytes, 0, cmdPayload, 4, xmlBytes.Length);
        } else { // "8byte" - standard
            // 8-byte framing: [u32 total_xml_len][u32 chunk_index=0][xml]
            cmdPayload = new byte[8 + xmlBytes.Length];
            cmdPayload[0] = (byte)(xmlBytes.Length & 0xFF);
            cmdPayload[1] = (byte)((xmlBytes.Length >> 8) & 0xFF);
            cmdPayload[2] = 0; cmdPayload[3] = 0;
            cmdPayload[4] = 0; cmdPayload[5] = 0; cmdPayload[6] = 0; cmdPayload[7] = 0;
            Array.Copy(xmlBytes, 0, cmdPayload, 8, xmlBytes.Length);
        }

        byte[] cmdPkt = MakePacket(0x2003, cmdPayload);
        Console.WriteLine("SEND SdkCmdAsk (" + cmdPkt.Length + " bytes total)");
        ns.Write(cmdPkt, 0, cmdPkt.Length);

        // Wait for response(s) - up to 10 seconds, read multiple packets
        DateTime deadline = DateTime.Now.AddSeconds(10);
        int count = 0;
        while (DateTime.Now < deadline && count < 5) {
            int remaining = (int)(deadline - DateTime.Now).TotalMilliseconds;
            if (remaining <= 0) break;
            try {
                byte[] resp = Recv(ns, remaining);
                if (resp == null) break;
                count++;
            } catch (IOException) { break; }
        }
        if (count == 0) Console.WriteLine("  (no response)");
        tcp.Close();
        Console.WriteLine();
    }
}
"@

[TcpTest]::Run("192.168.1.104", "8byte")
Start-Sleep -Milliseconds 500
[TcpTest]::Run("192.168.1.104", "4byte")
Start-Sleep -Milliseconds 500
[TcpTest]::Run("192.168.1.104", "raw")
