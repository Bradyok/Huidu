# Test port 10001 with ##GUID literal vs actual GUID for data methods
# Also: try rebooting device to reset port 9527

Add-Type -TypeDefinition @"
using System;
using System.Net.Sockets;
using System.Text;
using System.IO;
using System.Text.RegularExpressions;

public class P10001 {
    static TcpClient conn;
    static NetworkStream ns;
    static string sessionGuid = "##GUID";
    
    static byte[] Pkt(ushort cmd, byte[] pay) {
        int t=4+pay.Length; var b=new byte[t];
        b[0]=(byte)(t&0xFF);b[1]=(byte)((t>>8)&0xFF);
        b[2]=(byte)(cmd&0xFF);b[3]=(byte)((cmd>>8)&0xFF);
        Array.Copy(pay,0,b,4,pay.Length); return b;
    }
    static byte[] ReadFor(NetworkStream s, int ms) {
        var acc = new System.Collections.Generic.List<byte>();
        s.ReadTimeout = ms;
        var dl = DateTime.Now.AddMilliseconds(ms);
        var buf = new byte[65536];
        while (DateTime.Now < dl) {
            try {
                int n = s.Read(buf, 0, buf.Length);
                if (n == 0) break;
                for(int i=0;i<n;i++) acc.Add(buf[i]);
                // Check if we have complete packet
                if (acc.Count >= 4) {
                    int pktLen = acc[0]|(acc[1]<<8);
                    if (acc.Count >= pktLen) break;
                }
            } catch(IOException) { break; }
        }
        return acc.ToArray();
    }
    
    static string SendXml(string xml) {
        byte[] xb = Encoding.UTF8.GetBytes(xml);
        byte[] frame = new byte[8 + xb.Length];
        frame[0] = (byte)(xb.Length & 0xFF); frame[1] = (byte)((xb.Length >> 8) & 0xFF);
        Array.Copy(xb, 0, frame, 8, xb.Length);
        ns.Write(Pkt(0x2003, frame), 0, 4 + frame.Length);
        byte[] resp = ReadFor(ns, 5000);
        if (resp.Length > 12) {
            return Encoding.UTF8.GetString(resp, 12, Math.Min(resp.Length - 12, 2000));
        }
        return "(empty or short: " + resp.Length + " bytes)";
    }
    
    public static void Connect(string host) {
        conn = new TcpClient(); conn.Connect(host, 10001); ns = conn.GetStream();
        ns.Write(Pkt(0x2001, new byte[]{0,0,0,7}), 0, 8);
        ReadFor(ns, 1000); // SdkServiceAnswer
    }
    
    public static string GetIFVersion() {
        string xml = "<?xml version=\"1.0\" encoding=\"utf-8\"?><sdk guid=\"##GUID\"><in method=\"GetIFVersion\"><version value=\"1000000\"/></in></sdk>";
        string resp = SendXml(xml);
        // Extract session GUID from response
        var m = Regex.Match(resp, "sdk guid=\"([^\"]+)\"");
        if (m.Success) { sessionGuid = m.Groups[1].Value; }
        return resp;
    }
    
    public static string CallMethod(string method, string guid) {
        string xml = "<?xml version=\"1.0\" encoding=\"utf-8\"?><sdk guid=\"" + guid + "\"><in method=\"" + method + "\"/></sdk>";
        return SendXml(xml);
    }
    
    public static void Disconnect() { try{conn.Close();}catch{} }
}
"@

$host_ip = "192.168.1.104"

Write-Host "=== Port 10001 tests with different GUIDs ==="
[P10001]::Connect($host_ip)

Write-Host "GetIFVersion (sets session GUID):"
$r = [P10001]::GetIFVersion()
Write-Host $r.Substring(0, [Math]::Min($r.Length, 300))
Write-Host ""

# Try GetDeviceName with ##GUID (literal)
Write-Host "GetDeviceName with ##GUID literal:"
$r = [P10001]::CallMethod("GetDeviceName", "##GUID")
Write-Host $r.Substring(0, [Math]::Min($r.Length, 300))
Write-Host ""

# Try with the session GUID that GetIFVersion returned
Write-Host "GetDeviceName with session GUID from GetIFVersion:"
$r = [P10001]::CallMethod("GetDeviceName", "a336ae3e4cf3278438704c0e324f5dc5")
Write-Host $r.Substring(0, [Math]::Min($r.Length, 300))
Write-Host ""

# Try some other methods
Write-Host "GetFirewareVersion with ##GUID:"
$r = [P10001]::CallMethod("GetFirewareVersion", "##GUID")
Write-Host $r.Substring(0, [Math]::Min($r.Length, 300))
Write-Host ""

Write-Host "GetScreenInfo with ##GUID:"
$r = [P10001]::CallMethod("GetScreenInfo", "##GUID")
Write-Host $r.Substring(0, [Math]::Min($r.Length, 300))
Write-Host ""

Write-Host "GetEth0Info with ##GUID:"
$r = [P10001]::CallMethod("GetEth0Info", "##GUID")
Write-Host $r.Substring(0, [Math]::Min($r.Length, 300))
Write-Host ""

Write-Host "GetTimeInfo with ##GUID:"
$r = [P10001]::CallMethod("GetTimeInfo", "##GUID")
Write-Host $r.Substring(0, [Math]::Min($r.Length, 300))
Write-Host ""

Write-Host "GetLuminancePloy with ##GUID:"
$r = [P10001]::CallMethod("GetLuminancePloy", "##GUID")
Write-Host $r.Substring(0, [Math]::Min($r.Length, 300))
Write-Host ""

Write-Host "GetAllProgram with ##GUID:"
$r = [P10001]::CallMethod("GetAllProgram", "##GUID")
Write-Host $r.Substring(0, [Math]::Min($r.Length, 300))
Write-Host ""

Write-Host "OpenScreen with ##GUID:"
$r = [P10001]::CallMethod("OpenScreen", "##GUID")
Write-Host $r.Substring(0, [Math]::Min($r.Length, 300))
Write-Host ""

Write-Host "GetPlayStatus with ##GUID:"
$r = [P10001]::CallMethod("GetPlayStatus", "##GUID")
Write-Host $r.Substring(0, [Math]::Min($r.Length, 300))
Write-Host ""

[P10001]::Disconnect()
