# 1. Scan for open ports on the device (check web/upgrade ports)
# 2. Try different file types in FileStartAsk
# 3. Scan old protocol command ranges

Add-Type -TypeDefinition @"
using System;
using System.Net.Sockets;
using System.Text;
using System.IO;
using System.Net;

public class ScanTest {
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
    
    public static string CheckPort(string host, int port) {
        try {
            var tcp = new TcpClient();
            tcp.Connect(host, port);
            var ns = tcp.GetStream();
            // Try reading initial banner
            var buf = new byte[512];
            ns.ReadTimeout = 1000;
            int n = 0;
            try { n = ns.Read(buf, 0, buf.Length); } catch {}
            tcp.Close();
            if (n > 0) {
                string banner = System.Text.Encoding.ASCII.GetString(buf, 0, Math.Min(n, 80));
                banner = banner.Replace('\n', ' ').Replace('\r', ' ');
                return "OPEN, banner=" + banner;
            }
            return "OPEN (no banner)";
        } catch {
            return "closed";
        }
    }
    
    public static string CheckHttp(string host, int port) {
        try {
            var request = (System.Net.HttpWebRequest)System.Net.WebRequest.Create("http://" + host + ":" + port + "/");
            request.Timeout = 3000;
            request.Method = "GET";
            var response = (System.Net.HttpWebResponse)request.GetResponse();
            string result = "HTTP " + (int)response.StatusCode + " " + response.StatusDescription;
            response.Close();
            return result;
        } catch (System.Net.WebException e) {
            if (e.Response != null) {
                var resp = (System.Net.HttpWebResponse)e.Response;
                return "HTTP " + (int)resp.StatusCode + " " + resp.StatusDescription;
            }
            return "Error: " + e.Message.Substring(0, Math.Min(e.Message.Length, 60));
        }
    }
    
    public static string TestFileType(string host, ushort fileType) {
        try {
            var tcp = new TcpClient();
            tcp.Connect(host, 10001);
            var ns = tcp.GetStream();
            byte[] ver = new byte[] { 0x00, 0x00, 0x00, 0x07 };
            ns.Write(MakePkt(0x2001, ver), 0, MakePkt(0x2001, ver).Length);
            ReadRaw(ns, 500);
            
            // FileStartAsk with different file type
            byte[] md5 = System.Text.Encoding.ASCII.GetBytes("00000000000000000000000000000001");
            byte[] fsize = BitConverter.GetBytes((ulong)10);
            byte[] ftype = BitConverter.GetBytes(fileType);
            byte[] fname = System.Text.Encoding.ASCII.GetBytes("fw.bin\0");
            byte[] payload = new byte[32 + 8 + 2 + fname.Length];
            Array.Copy(md5, payload, 32);
            Array.Copy(fsize, 0, payload, 32, 8);
            Array.Copy(ftype, 0, payload, 40, 2);
            Array.Copy(fname, 0, payload, 42, fname.Length);
            ns.Write(MakePkt(0x8001, payload), 0, MakePkt(0x8001, payload).Length);
            byte[] resp = ReadRaw(ns, 2000);
            tcp.Close();
            if (resp.Length >= 6) {
                ushort cmd = (ushort)(resp[2] | (resp[3] << 8));
                int err = resp[4] | (resp[5] << 8);
                return "type=" + fileType + " -> cmd=0x" + cmd.ToString("X4") + " err=" + err;
            }
            return "type=" + fileType + " -> no response";
        } catch (Exception ex) {
            return "type=" + fileType + " -> ERROR: " + ex.Message;
        }
    }
    
    public static string TestOldCmd(string host, ushort cmd_code) {
        try {
            var tcp = new TcpClient();
            tcp.Connect(host, 10001);
            var ns = tcp.GetStream();
            // Don't do SDK handshake - send old cmd directly
            byte[] pkt = MakePkt(cmd_code, new byte[0]);
            ns.Write(pkt, 0, pkt.Length);
            byte[] resp = ReadRaw(ns, 1000);
            tcp.Close();
            if (resp.Length >= 4) {
                ushort rcmd = (ushort)(resp[2] | (resp[3] << 8));
                string payload = (resp.Length > 4) ? BitConverter.ToString(resp, 4, Math.Min(resp.Length-4, 8)) : "(none)";
                return "cmd=0x" + cmd_code.ToString("X4") + " -> resp_cmd=0x" + rcmd.ToString("X4") + " " + payload;
            }
            return "cmd=0x" + cmd_code.ToString("X4") + " -> no response (" + resp.Length + "b)";
        } catch (Exception ex) {
            return "cmd=0x" + cmd_code.ToString("X4") + " -> ERROR: " + ex.Message;
        }
    }
}
"@

$host_ip = "192.168.1.104"

Write-Host "=== Port scan ==="
foreach ($port in @(80, 443, 8080, 8888, 9000, 9001, 9527, 10000, 10001, 10002, 10080, 12300)) {
    $result = [ScanTest]::CheckPort($host_ip, $port)
    if ($result -notlike "closed") {
        Write-Host "  Port ${port}: $result"
    }
}

Write-Host ""
Write-Host "=== HTTP interface scan ==="
foreach ($port in @(80, 8080, 8888, 10080)) {
    $result = [ScanTest]::CheckHttp($host_ip, $port)
    if ($result -notlike "*Connect*" -and $result -notlike "*timed*") {
        Write-Host "  HTTP:${port} -> $result"
    }
}

Write-Host ""
Write-Host "=== File type scan (0x8001) ==="
foreach ($ftype in @(0, 1, 2, 3, 4, 5, 10, 100, 0xFF, 0x1000)) {
    Write-Host ([ScanTest]::TestFileType($host_ip, $ftype))
    Start-Sleep -Milliseconds 100
}

Write-Host ""
Write-Host "=== Old protocol command scan (no handshake) ==="
foreach ($cmd in @(0x0001, 0x0002, 0x0003, 0x0010, 0x0011, 0x0100, 0x0101, 0x1001, 0x1002, 0x1003)) {
    $result = [ScanTest]::TestOldCmd($host_ip, $cmd)
    if ($result -notlike "*no response*") {
        Write-Host "  $result"
    }
}
