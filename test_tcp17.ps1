# USE SESSION GUID returned by GetIFVersion for all subsequent calls
# Also note: response uses result="kSuccess" attribute on <out>, not <result value="0"/>

Add-Type -TypeDefinition @"
using System;
using System.Net.Sockets;
using System.Text;
using System.IO;
using System.Text.RegularExpressions;

public class Proto17 {
    static byte[] MakePkt(ushort cmd, byte[] payload) {
        int total = 4 + payload.Length;
        byte[] pkt = new byte[total];
        pkt[0]=(byte)(total&0xFF); pkt[1]=(byte)((total>>8)&0xFF);
        pkt[2]=(byte)(cmd&0xFF); pkt[3]=(byte)((cmd>>8)&0xFF);
        Array.Copy(payload, 0, pkt, 4, payload.Length);
        return pkt;
    }
    static byte[] ReadRaw(NetworkStream ns, int ms) {
        var acc = new System.Collections.Generic.List<byte>();
        var buf = new byte[65536]; ns.ReadTimeout = 300;
        var dl = DateTime.Now.AddMilliseconds(ms);
        while (DateTime.Now < dl) {
            try { int n = ns.Read(buf, 0, buf.Length); if(n==0) break; for(int i=0;i<n;i++) acc.Add(buf[i]); } catch(IOException){}
        }
        return acc.ToArray();
    }
    static TcpClient conn; static NetworkStream ns_global;
    static string sessionGuid = "A1B2C3D4-E5F6-7890-ABCD-EF1234567890";

    public static string Connect(string host) {
        conn = new TcpClient(); conn.Connect(host, 10001); ns_global = conn.GetStream();
        byte[] ver = new byte[] { 0x00, 0x00, 0x00, 0x07 };
        ns_global.Write(MakePkt(0x2001, ver), 0, MakePkt(0x2001, ver).Length);
        byte[] r = ReadRaw(ns_global, 1000);
        return r.Length>=4 ? "SdkServiceAnswer: device_version="+BitConverter.ToString(r,4) : "no_resp";
    }
    public static void Disconnect() { if(conn!=null){try{conn.Close();}catch{}}}

    static string SendXml(string xmlBody) {
        byte[] xmlBytes = Encoding.UTF8.GetBytes(xmlBody);
        byte[] framing = new byte[8+xmlBytes.Length];
        framing[0]=(byte)(xmlBytes.Length&0xFF); framing[1]=(byte)((xmlBytes.Length>>8)&0xFF);
        Array.Copy(xmlBytes, 0, framing, 8, xmlBytes.Length);
        ns_global.Write(MakePkt(0x2003, framing), 0, MakePkt(0x2003, framing).Length);
        byte[] resp = ReadRaw(ns_global, 3000);
        if (resp.Length >= 6) {
            ushort cmd = (ushort)(resp[2]|(resp[3]<<8));
            int err = resp[4]|(resp[5]<<8);
            if (cmd == 0x2004 && resp.Length > 12) {
                return "OK:" + Encoding.UTF8.GetString(resp, 12, Math.Min(resp.Length-12, 600));
            }
            return "ERR:" + err;
        }
        return "NORESPONSE";
    }
    
    public static string GetIFVersionAndRegister() {
        string xml = "<?xml version=\"1.0\" encoding=\"utf-8\"?><sdk guid=\""+sessionGuid+"\"><in method=\"GetIFVersion\"><version value=\"1000000\"/></in></sdk>";
        string result = SendXml(xml);
        if (result.StartsWith("OK:")) {
            // Extract the session GUID from response: <sdk guid="...">
            var m = Regex.Match(result, @"<sdk guid=""([^""]+)""");
            if (m.Success) {
                sessionGuid = m.Groups[1].Value;
                Console.WriteLine("Session GUID: " + sessionGuid);
            }
        }
        return result;
    }
    
    public static string CallMethod(string method, string body="") {
        string xml = "<?xml version=\"1.0\" encoding=\"utf-8\"?><sdk guid=\""+sessionGuid+"\"><in method=\""+method+"\">"+body+"</in></sdk>";
        return SendXml(xml);
    }
}
"@

$host_ip = "192.168.1.104"
Write-Host ([Proto17]::Connect($host_ip))

Write-Host ""
Write-Host "=== Step 1: GetIFVersion to get session GUID ==="
$r = [Proto17]::GetIFVersionAndRegister()
Write-Host "GetIFVersion result: $($r.Substring(0, [Math]::Min($r.Length, 300)))"

Write-Host ""
Write-Host "=== Step 2: Call methods with session GUID ==="
$methods_to_test = @(
    @{name="GetDeviceInfo"; body=""},
    @{name="GetAllProgram"; body=""},
    @{name="GetLuminancePloy"; body=""},
    @{name="GetSwitchTime"; body=""},
    @{name="GetTimeInfo"; body=""},
    @{name="GetEth0Info"; body=""},
    @{name="GetWifiInfo"; body=""},
    @{name="GetBootLogo"; body=""},
    @{name="GetUpgradeResult"; body=""},
    @{name="GetScreenInfo"; body=""},
    @{name="GetDeviceID"; body=""},
    @{name="GetDeviceName"; body=""},
    @{name="GetFirewareVersion"; body=""},
    @{name="OpenScreen"; body=""},
    @{name="CloseScreen"; body=""},
    @{name="GetAllProgram"; body=""},
    @{name="GetLicense"; body=""}
)

foreach ($m in $methods_to_test) {
    $r = [Proto17]::CallMethod($m.name, $m.body)
    if ($r.StartsWith("OK:")) {
        Write-Host "[OK] $($m.name): $($r.Substring(3, [Math]::Min($r.Length-3, 300)))"
    } else {
        Write-Host "[ERR] $($m.name): $r"
    }
}

[Proto17]::Disconnect()
