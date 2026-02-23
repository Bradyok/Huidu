# GetIFVersion version negotiation - find the correct version value
# Then see what happens with other methods after successful GetIFVersion

Add-Type -TypeDefinition @"
using System;
using System.Net.Sockets;
using System.Text;
using System.IO;

public class Proto16 {
    static byte[] MakePkt(ushort cmd, byte[] payload) {
        int total = 4 + payload.Length;
        byte[] pkt = new byte[total];
        pkt[0] = (byte)(total & 0xFF); pkt[1] = (byte)((total>>8)&0xFF);
        pkt[2] = (byte)(cmd&0xFF); pkt[3] = (byte)((cmd>>8)&0xFF);
        Array.Copy(payload, 0, pkt, 4, payload.Length);
        return pkt;
    }
    static byte[] ReadRaw(NetworkStream ns, int ms) {
        var acc = new System.Collections.Generic.List<byte>();
        var buf = new byte[65536]; ns.ReadTimeout = 300;
        var dl = DateTime.Now.AddMilliseconds(ms);
        while (DateTime.Now < dl) {
            try { int n = ns.Read(buf, 0, buf.Length); if (n==0) break; for(int i=0;i<n;i++) acc.Add(buf[i]); } catch (IOException) {}
        }
        return acc.ToArray();
    }
    static TcpClient conn; static NetworkStream ns_global;
    static string guid = "A1B2C3D4-E5F6-7890-ABCD-EF1234567890";

    public static string Connect(string host) {
        conn = new TcpClient(); conn.Connect(host, 10001); ns_global = conn.GetStream();
        byte[] ver = new byte[] { 0x00, 0x00, 0x00, 0x07 };
        ns_global.Write(MakePkt(0x2001, ver), 0, MakePkt(0x2001, ver).Length);
        byte[] r = ReadRaw(ns_global, 1000);
        return r.Length>=4 ? "0x"+((ushort)(r[2]|(r[3]<<8))).ToString("X4") : "no_resp";
    }
    public static void Disconnect() { if(conn!=null){try{conn.Close();}catch{}}}

    public static string TestGetIFVersion(long versionInt) {
        string xml = "<?xml version=\"1.0\" encoding=\"utf-8\"?><sdk guid=\""+guid+"\"><in method=\"GetIFVersion\"><version value=\""+versionInt+"\"/></in></sdk>";
        byte[] xmlBytes = Encoding.UTF8.GetBytes(xml);
        byte[] framing = new byte[8+xmlBytes.Length];
        framing[0]=(byte)(xmlBytes.Length&0xFF); framing[1]=(byte)((xmlBytes.Length>>8)&0xFF);
        Array.Copy(xmlBytes, 0, framing, 8, xmlBytes.Length);
        ns_global.Write(MakePkt(0x2003, framing), 0, MakePkt(0x2003, framing).Length);
        byte[] resp = ReadRaw(ns_global, 2000);
        if (resp.Length >= 6) {
            ushort cmd = (ushort)(resp[2]|(resp[3]<<8));
            int err = resp[4]|(resp[5]<<8);
            if (cmd == 0x2004) {
                string x = (resp.Length>12) ? Encoding.UTF8.GetString(resp,12,Math.Min(resp.Length-12,400)) : "(short)";
                return "[OK] v="+versionInt+" -> "+x;
            }
            return "["+err+"] v="+versionInt;
        }
        return "[---] v="+versionInt+" ("+resp.Length+"b)";
    }
    
    public static string TestMethod(string method) {
        string xml = "<?xml version=\"1.0\" encoding=\"utf-8\"?><sdk guid=\""+guid+"\"><in method=\""+method+"\"/></sdk>";
        byte[] xmlBytes = Encoding.UTF8.GetBytes(xml);
        byte[] framing = new byte[8+xmlBytes.Length];
        framing[0]=(byte)(xmlBytes.Length&0xFF); framing[1]=(byte)((xmlBytes.Length>>8)&0xFF);
        Array.Copy(xmlBytes, 0, framing, 8, xmlBytes.Length);
        ns_global.Write(MakePkt(0x2003, framing), 0, MakePkt(0x2003, framing).Length);
        byte[] resp = ReadRaw(ns_global, 2000);
        if (resp.Length >= 6) {
            ushort cmd = (ushort)(resp[2]|(resp[3]<<8));
            int err = resp[4]|(resp[5]<<8);
            if (cmd == 0x2004) {
                string x = (resp.Length>12) ? Encoding.UTF8.GetString(resp,12,Math.Min(resp.Length-12,300)) : "(short)";
                return "[OK] "+method+" -> "+x;
            }
            return "["+err+"] "+method;
        }
        return "[---] "+method+" ("+resp.Length+"b)";
    }
}
"@

$host_ip = "192.168.1.104"

Write-Host "=== GetIFVersion version parameter scan ==="
Write-Host ([Proto16]::Connect($host_ip))
foreach ($v in @(0, 1, 100, 1000, 10000, 100000, 1000000, 1000001, 2000000, 3000000, 5000000, 7000000, 10000000, 100000000)) {
    Write-Host ([Proto16]::TestGetIFVersion($v))
}
[Proto16]::Disconnect()

Start-Sleep -Milliseconds 300

Write-Host ""
Write-Host "=== After successful GetIFVersion: try other methods ==="
Write-Host ([Proto16]::Connect($host_ip))

# First call GetIFVersion with the right version
# (We'll try 1000000 and see if methods unlock)
Write-Host "  GetIFVersion(1000000): $([Proto16]::TestGetIFVersion(1000000))"
Write-Host "  GetIFVersion(1000001): $([Proto16]::TestGetIFVersion(1000001))"
Write-Host "  GetDeviceInfo: $([Proto16]::TestMethod("GetDeviceInfo"))"
Write-Host "  OpenScreen: $([Proto16]::TestMethod("OpenScreen"))"
Write-Host "  GetAllProgram: $([Proto16]::TestMethod("GetAllProgram"))"
[Proto16]::Disconnect()
