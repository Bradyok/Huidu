# 1. Old protocol commands AFTER SDK handshake
# 2. Precise version threshold test  
# 3. GetIFVersion with body params

Add-Type -TypeDefinition @"
using System;
using System.Net.Sockets;
using System.Text;
using System.IO;

public class Proto15 {
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
    static TcpClient conn;
    static NetworkStream ns_global;

    public static string Connect(string host) {
        conn = new TcpClient();
        conn.Connect(host, 10001);
        ns_global = conn.GetStream();
        byte[] ver = new byte[] { 0x00, 0x00, 0x00, 0x07 };
        ns_global.Write(MakePkt(0x2001, ver), 0, MakePkt(0x2001, ver).Length);
        byte[] r = ReadRaw(ns_global, 1000);
        return "Connected: " + (r.Length>=4 ? "cmd=0x"+((ushort)(r[2]|(r[3]<<8))).ToString("X4") : "no resp");
    }
    public static void Disconnect() { if(conn!=null){try{conn.Close();}catch{}}}
    
    public static string SendRaw(ushort cmd_code, byte[] payload) {
        byte[] pkt = MakePkt(cmd_code, payload);
        ns_global.Write(pkt, 0, pkt.Length);
        byte[] resp = ReadRaw(ns_global, 1500);
        if (resp.Length >= 4) {
            ushort rcmd = (ushort)(resp[2] | (resp[3] << 8));
            string pay = (resp.Length > 4) ? BitConverter.ToString(resp, 4, Math.Min(resp.Length-4, 20)) : "(none)";
            return "0x"+cmd_code.ToString("X4")+" -> cmd=0x"+rcmd.ToString("X4")+" "+pay;
        }
        return "0x"+cmd_code.ToString("X4")+" -> no response";
    }
    
    public static string TestVersion(string host, uint version) {
        try {
            var tcp = new TcpClient(); tcp.Connect(host, 10001);
            var ns = tcp.GetStream();
            byte[] vb = BitConverter.GetBytes(version);
            ns.Write(MakePkt(0x2001, vb), 0, MakePkt(0x2001, vb).Length);
            byte[] r = ReadRaw(ns, 1000); tcp.Close();
            if (r.Length >= 4) {
                ushort cmd = (ushort)(r[2]|(r[3]<<8));
                int err = (r.Length>=6) ? (r[4]|(r[5]<<8)) : -1;
                return "v0x"+version.ToString("X8")+" -> cmd=0x"+cmd.ToString("X4")+(cmd==0x2000?" err="+err:"");
            }
            return "v0x"+version.ToString("X8")+" -> no response";
        } catch (Exception ex) { return "v0x"+version.ToString("X8")+" -> ERROR: "+ex.Message; }
    }
    
    public static string TestXmlMethod(string method, string body) {
        string guid = "AAAABBBB-CCCC-DDDD-EEEE-FFFFAAAABBBB";
        string xml = "<?xml version=\"1.0\" encoding=\"utf-8\"?><sdk guid=\""+guid+"\"><in method=\""+method+"\">"+body+"</in></sdk>";
        byte[] xmlBytes = Encoding.UTF8.GetBytes(xml);
        byte[] framing = new byte[8 + xmlBytes.Length];
        framing[0] = (byte)(xmlBytes.Length & 0xFF);
        framing[1] = (byte)((xmlBytes.Length >> 8) & 0xFF);
        Array.Copy(xmlBytes, 0, framing, 8, xmlBytes.Length);
        ns_global.Write(MakePkt(0x2003, framing), 0, MakePkt(0x2003, framing).Length);
        byte[] resp = ReadRaw(ns_global, 2000);
        if (resp.Length >= 6) {
            ushort cmd = (ushort)(resp[2]|(resp[3]<<8));
            int err = resp[4]|(resp[5]<<8);
            if (cmd == 0x2004) {
                string x = (resp.Length>12) ? Encoding.UTF8.GetString(resp,12,Math.Min(resp.Length-12,400)) : "(short)";
                return "[OK] "+method+" body=\""+body+"\" -> "+x;
            }
            return "["+err+"] "+method+" body=\""+body.Substring(0,Math.Min(body.Length,30))+"\"";
        }
        return "[---] "+method+" body=\""+body+"\"";
    }
}
"@

$host_ip = "192.168.1.104"

Write-Host "=== Precise version threshold ==="
foreach ($v in @(0x01000001, 0x01000002, 0x01000005, 0x01000006, 0x01000007, 0x01000008)) {
    Write-Host ([Proto15]::TestVersion($host_ip, $v))
    Start-Sleep -Milliseconds 100
}

Write-Host ""
Write-Host "=== Old binary commands after SDK handshake ==="
Write-Host ([Proto15]::Connect($host_ip))
# Try various old protocol command ranges AFTER handshake
$cmds = @(
    0x0001, 0x0002, 0x0003, 0x0004, 0x0005,
    0x0010, 0x0020, 0x0030, 0x0040, 0x0050,
    0x0060, 0x0061, 0x0062, 0x0070, 0x0080,
    0x1001, 0x1002, 0x1003,
    0x3001, 0x3002,  # FPGA
    0x4001, 0x4002,  # Screen test
    0x5001, 0x5002   # Boot screen
)
foreach ($cmd in $cmds) {
    $result = [Proto15]::SendRaw($cmd, @())
    if ($result -notlike "*no response*") {
        Write-Host "  $result"
    }
}
[Proto15]::Disconnect()

Write-Host ""
Write-Host "=== GetIFVersion with body params ==="
Write-Host ([Proto15]::Connect($host_ip))
# Try GetIFVersion with various XML bodies
@(
    "",
    "<version value=""1""/>",
    "<version/>",
    "<type value=""1""/>",
    "<ifVersion value=""1000001""/>",
    "<data/>",
    " "
) | ForEach-Object {
    Write-Host ([Proto15]::TestXmlMethod("GetIFVersion", $_))
}
[Proto15]::Disconnect()
