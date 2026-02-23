# Test double-quoted XML format vs single-quoted
# Also tests XML without declaration

Add-Type -TypeDefinition @"
using System;
using System.Net.Sockets;
using System.Text;
using System.IO;

public class XmlTest {
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
    static TcpClient conn = null;
    static NetworkStream ns_global = null;

    public static string Connect(string host) {
        conn = new TcpClient();
        conn.Connect(host, 10001);
        ns_global = conn.GetStream();
        byte[] ver = new byte[] { 0x00, 0x00, 0x00, 0x07 };
        byte[] svcPkt = MakePkt(0x2001, ver);
        ns_global.Write(svcPkt, 0, svcPkt.Length);
        byte[] svcResp = ReadRaw(ns_global, 2000);
        if (svcResp.Length >= 4) {
            ushort cmd = (ushort)(svcResp[2] | (svcResp[3] << 8));
            string payload = (svcResp.Length > 4) ? BitConverter.ToString(svcResp, 4) : "(none)";
            return "SdkService: cmd=0x" + cmd.ToString("X4") + " payload=" + payload;
        }
        return "No SdkService response";
    }
    public static void Disconnect() {
        if (conn != null) { try { conn.Close(); } catch {} conn = null; }
    }
    public static string TestXml(string label, string xml) {
        byte[] xmlBytes = Encoding.UTF8.GetBytes(xml);
        byte[] framing = new byte[8 + xmlBytes.Length];
        framing[0] = (byte)(xmlBytes.Length & 0xFF);
        framing[1] = (byte)((xmlBytes.Length >> 8) & 0xFF);
        Array.Copy(xmlBytes, 0, framing, 8, xmlBytes.Length);
        var pkt = MakePkt(0x2003, framing);
        ns_global.Write(pkt, 0, pkt.Length);
        byte[] resp = ReadRaw(ns_global, 3000);
        if (resp.Length >= 6) {
            ushort cmd = (ushort)(resp[2] | (resp[3] << 8));
            int errCode = resp[4] | (resp[5] << 8);
            if (cmd == 0x2004) {
                string xml_resp = (resp.Length > 12) ? Encoding.UTF8.GetString(resp, 12, Math.Min(resp.Length-12, 200)) : "(short)";
                return "[OK] " + label + " -> cmd=0x2004 xml=" + xml_resp;
            } else {
                return "[ERR] " + label + " -> cmd=0x" + cmd.ToString("X4") + " err=" + errCode;
            }
        }
        return "[---] " + label + " -> no response (" + resp.Length + " bytes)";
    }
}
"@

$host_ip = "192.168.1.104"
$guid = "12345678-1234-1234-1234-123456789ABC"

Write-Host ([XmlTest]::Connect($host_ip))
Write-Host ""

# Test 1: double-quoted declaration (matches device DLL format)
$xml1 = "<?xml version=`"1.0`" encoding=`"utf-8`"?><sdk guid=`"$guid`"><in method=`"GetDeviceInfo`"/></sdk>"
Write-Host ([XmlTest]::TestXml("DQ-decl, self-close", $xml1))

# Test 2: no XML declaration
$xml2 = "<sdk guid=`"$guid`"><in method=`"GetDeviceInfo`"/></sdk>"
Write-Host ([XmlTest]::TestXml("No decl, self-close", $xml2))

# Test 3: double-quoted, separate close tag  
$xml3 = "<?xml version=`"1.0`" encoding=`"utf-8`"?><sdk guid=`"$guid`"><in method=`"GetDeviceInfo`"></in></sdk>"
Write-Host ([XmlTest]::TestXml("DQ-decl, open+close", $xml3))

# Test 4: single-quoted (original format)
$xml4 = "<?xml version='1.0' encoding='utf-8'?><sdk guid=`"$guid`"><in method=`"GetDeviceInfo`"></in></sdk>"
Write-Host ([XmlTest]::TestXml("SQ-decl, open+close", $xml4))

# Test 5: GetIFVersion with double-quoted
$xml5 = "<?xml version=`"1.0`" encoding=`"utf-8`"?><sdk guid=`"$guid`"><in method=`"GetIFVersion`"/></sdk>"
Write-Host ([XmlTest]::TestXml("DQ GetIFVersion", $xml5))

# Test 6: Try "Reboot" method (simple, no params)
$xml6 = "<?xml version=`"1.0`" encoding=`"utf-8`"?><sdk guid=`"$guid`"><in method=`"Reboot`"/></sdk>"
Write-Host ([XmlTest]::TestXml("DQ Reboot (don't execute - just test)", $xml6))

# Test 7: OpenScreen
$xml7 = "<?xml version=`"1.0`" encoding=`"utf-8`"?><sdk guid=`"$guid`"><in method=`"OpenScreen`"/></sdk>"
Write-Host ([XmlTest]::TestXml("DQ OpenScreen", $xml7))

# Test 8: GetScreenInfo
$xml8 = "<?xml version=`"1.0`" encoding=`"utf-8`"?><sdk guid=`"$guid`"><in method=`"GetScreenInfo`"/></sdk>"
Write-Host ([XmlTest]::TestXml("DQ GetScreenInfo", $xml8))

[XmlTest]::Disconnect()
