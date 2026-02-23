# Test binary file transfer protocol (0x8001-0x8006) - does old firmware support it?
# Also test SdkServiceAsk with versions between 0x01 and 0x07 to find exact threshold

Add-Type -TypeDefinition @"
using System;
using System.Net.Sockets;
using System.Text;
using System.IO;

public class ProtoTest {
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
        var buf = new byte[65536];
        ns.ReadTimeout = 300;
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
    
    public static string TestVersion(string host, uint version) {
        try {
            var tcp = new TcpClient();
            tcp.Connect(host, 10001);
            var ns = tcp.GetStream();
            byte[] vBytes = new byte[] {
                (byte)(version & 0xFF),
                (byte)((version>>8) & 0xFF),
                (byte)((version>>16) & 0xFF),
                (byte)((version>>24) & 0xFF)
            };
            ns.Write(MakePkt(0x2001, vBytes), 0, MakePkt(0x2001, vBytes).Length);
            byte[] resp = ReadRaw(ns, 2000);
            tcp.Close();
            if (resp.Length >= 6) {
                ushort cmd = (ushort)(resp[2] | (resp[3] << 8));
                string payload = BitConverter.ToString(resp, 4, Math.Min(resp.Length-4, 8));
                return "v0x" + version.ToString("X8") + " -> cmd=0x" + cmd.ToString("X4") + " payload=" + payload;
            }
            return "v0x" + version.ToString("X8") + " -> no/short response (" + resp.Length + " bytes)";
        } catch (Exception ex) {
            return "v0x" + version.ToString("X8") + " -> ERROR: " + ex.Message;
        }
    }
    
    public static string TestFileTransfer(string host) {
        try {
            var tcp = new TcpClient();
            tcp.Connect(host, 10001);
            var ns = tcp.GetStream();
            
            // First do version handshake
            byte[] ver = new byte[] { 0x00, 0x00, 0x00, 0x07 };
            ns.Write(MakePkt(0x2001, ver), 0, MakePkt(0x2001, ver).Length);
            ReadRaw(ns, 1000); // consume handshake response
            
            // Now try FileStartAsk (0x8001)
            // Payload: [32 bytes MD5 hex][u64 filesize][u16 filetype][filename\0]
            var md5_hex = Encoding.ASCII.GetBytes("00000000000000000000000000000001"); // 32 bytes
            byte[] file_size = BitConverter.GetBytes((ulong)100); // 100 bytes
            byte[] file_type = BitConverter.GetBytes((ushort)1); // type 1
            byte[] filename = Encoding.ASCII.GetBytes("test.txt\0");
            
            byte[] start_payload = new byte[md5_hex.Length + file_size.Length + file_type.Length + filename.Length];
            Array.Copy(md5_hex, 0, start_payload, 0, md5_hex.Length);
            Array.Copy(file_size, 0, start_payload, 32, file_size.Length);
            Array.Copy(file_type, 0, start_payload, 40, file_type.Length);
            Array.Copy(filename, 0, start_payload, 42, filename.Length);
            
            ns.Write(MakePkt(0x8001, start_payload), 0, MakePkt(0x8001, start_payload).Length);
            byte[] resp = ReadRaw(ns, 3000);
            tcp.Close();
            
            if (resp.Length >= 4) {
                ushort cmd = (ushort)(resp[2] | (resp[3] << 8));
                string payload = (resp.Length > 4) ? BitConverter.ToString(resp, 4, Math.Min(resp.Length-4, 12)) : "(none)";
                return "FileStartAsk -> cmd=0x" + cmd.ToString("X4") + " payload=" + payload;
            }
            return "FileStartAsk -> no response (" + resp.Length + " bytes)";
        } catch (Exception ex) {
            return "FileStartAsk -> ERROR: " + ex.Message;
        }
    }
}
"@

$host_ip = "192.168.1.104"

Write-Host "=== Version threshold scan ==="
# We know 0x01000000 fails and 0x07000000 works. Binary search the threshold.
foreach ($v in @(0x02000000, 0x03000000, 0x04000000, 0x05000000, 0x06000000, 0x01010000, 0x01020000, 0x01030000)) {
    Write-Host ([ProtoTest]::TestVersion($host_ip, $v))
    Start-Sleep -Milliseconds 200
}

Write-Host ""
Write-Host "=== File transfer test ==="
Write-Host ([ProtoTest]::TestFileTransfer($host_ip))
