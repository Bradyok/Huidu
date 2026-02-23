# Test multiple method names with correct SDK version (0x07000000)
# Version 0x07000000 gets us SdkServiceAnswer (cmd=0x2002) ✓

Add-Type -TypeDefinition @"
using System;
using System.Net.Sockets;
using System.Text;
using System.IO;

public class MethodTest {
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
                // If we have data and a pause, keep reading
            } catch (IOException) {}
        }
        return acc.ToArray();
    }

    static TcpClient conn = null;
    static NetworkStream ns_global = null;

    public static void Connect(string host) {
        conn = new TcpClient();
        conn.Connect(host, 10001);
        ns_global = conn.GetStream();
        Console.WriteLine("Connected to " + host);

        // Send SdkServiceAsk with version 0x07000000
        byte[] ver = new byte[] { 0x00, 0x00, 0x00, 0x07 }; // LE: 0x07000000
        byte[] svcPkt = MakePkt(0x2001, ver);
        ns_global.Write(svcPkt, 0, svcPkt.Length);

        byte[] svcResp = ReadRaw(ns_global, 2000);
        if (svcResp.Length >= 4) {
            ushort cmd = (ushort)(svcResp[2] | (svcResp[3] << 8));
            Console.WriteLine("SdkService: cmd=0x" + cmd.ToString("X4") + " payload=" + BitConverter.ToString(svcResp, 4, Math.Max(0, svcResp.Length-4)));
        }
    }

    public static void Disconnect() {
        if (conn != null) { try { conn.Close(); } catch {} conn = null; }
    }

    public static void TestMethod(string methodName) {
        string guid = "CCCCCCCC-CCCC-CCCC-CCCC-CCCCCCCCCCCC";
        string xml = "<?xml version='1.0' encoding='utf-8'?><sdk guid=\"" + guid + "\"><in method=\"" + methodName + "\"></in></sdk>";
        byte[] xmlBytes = Encoding.UTF8.GetBytes(xml);
        byte[] framing = new byte[8 + xmlBytes.Length];
        framing[0] = (byte)(xmlBytes.Length & 0xFF);
        framing[1] = (byte)((xmlBytes.Length >> 8) & 0xFF);
        Array.Copy(xmlBytes, 0, framing, 8, xmlBytes.Length);
        var pkt = MakePkt(0x2003, framing);
        ns_global.Write(pkt, 0, pkt.Length);

        byte[] resp = ReadRaw(ns_global, 5000);
        if (resp.Length >= 4) {
            ushort cmd = (ushort)(resp[2] | (resp[3] << 8));
            if (cmd == 0x2004) {
                // Success - SdkCmdAnswer
                Console.WriteLine("  [OK] " + methodName + ": cmd=0x2004, resp_len=" + resp.Length);
                if (resp.Length > 12) {
                    int xmlLen = resp[4] | (resp[5] << 8) | (resp[6] << 16) | (resp[7] << 24);
                    Console.WriteLine("  XML_LEN=" + xmlLen + " data: " + Encoding.UTF8.GetString(resp, 12, Math.Min(resp.Length-12, 400)));
                }
            } else if (cmd == 0x2000) {
                // Error
                int errCode = (resp.Length >= 6) ? (resp[4] | (resp[5] << 8)) : -1;
                Console.WriteLine("  [ERR] " + methodName + ": error_code=" + errCode + " (0x" + errCode.ToString("X") + ")");
            } else {
                Console.WriteLine("  [???] " + methodName + ": cmd=0x" + cmd.ToString("X4") + " bytes=" + BitConverter.ToString(resp));
            }
        } else {
            Console.WriteLine("  [---] " + methodName + ": no response (" + resp.Length + " bytes)");
        }
    }
}
"@

[MethodTest]::Connect("192.168.1.104")

# Test all known method names
$methods = @(
    "GetIFVersion",
    "GetDeviceInfo",
    "GetHardwareInfo",
    "GetAllProgram",
    "GetSDKTcpServer",
    "GetAdminModeInfo",
    "GetLuminancePloy",
    "GetSwitchTime",
    "GetTimeInfo",
    "GetEth0Info",
    "GetNetworkInfo",
    "GetWifiInfo",
    "GetPppoeInfo",
    "GetLicense",
    "GetUpgradeResult",
    "GetBootLogo",
    "GetFileChecklist",
    "GetHardwareInfo",
    "GetScreenRotation",
    "GetBoxHwConfig",
    "GetSDKFPGAConfig",
    "GetCurrentPlayProgramGUID",
    "OpenScreen",
    "CloseScreen",
    "RebootDevice"
)

foreach ($m in $methods) {
    [MethodTest]::TestMethod($m)
    Start-Sleep -Milliseconds 100
}

[MethodTest]::Disconnect()
