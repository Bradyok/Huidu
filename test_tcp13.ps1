# Test exact version threshold and try ALL the methods from BOXPLAYER_DECOMPILATION list
# Also test with ALL the method names we know about

Add-Type -TypeDefinition @"
using System;
using System.Net.Sockets;
using System.Text;
using System.IO;

public class ProtoTest2 {
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
        byte[] svcResp = ReadRaw(ns_global, 2000);
        if (svcResp.Length >= 4) {
            ushort cmd = (ushort)(svcResp[2] | (svcResp[3] << 8));
            return "Connected: cmd=0x" + cmd.ToString("X4") + " payload=" + (svcResp.Length>4 ? BitConverter.ToString(svcResp,4) : "(none)");
        }
        return "Connected but no response";
    }
    public static void Disconnect() {
        if (conn != null) { try { conn.Close(); } catch {} }
    }
    public static string TestMethod(string method) {
        string guid = "11111111-1111-1111-1111-111111111111";
        string xml = "<?xml version=\"1.0\" encoding=\"utf-8\"?><sdk guid=\"" + guid + "\"><in method=\"" + method + "\"/></sdk>";
        byte[] xmlBytes = Encoding.UTF8.GetBytes(xml);
        byte[] framing = new byte[8 + xmlBytes.Length];
        framing[0] = (byte)(xmlBytes.Length & 0xFF);
        framing[1] = (byte)((xmlBytes.Length >> 8) & 0xFF);
        Array.Copy(xmlBytes, 0, framing, 8, xmlBytes.Length);
        ns_global.Write(MakePkt(0x2003, framing), 0, MakePkt(0x2003, framing).Length);
        byte[] resp = ReadRaw(ns_global, 2000);
        if (resp.Length >= 6) {
            ushort cmd = (ushort)(resp[2] | (resp[3] << 8));
            int err = resp[4] | (resp[5] << 8);
            if (cmd == 0x2004) {
                string xmlResp = (resp.Length > 12) ? Encoding.UTF8.GetString(resp, 12, Math.Min(resp.Length-12, 300)) : "(short)";
                return "[OK] " + method + " -> " + xmlResp;
            }
            return "[" + err + "] " + method;
        }
        return "[---] " + method + " (no response " + resp.Length + "b)";
    }
}
"@

$host_ip = "192.168.1.104"
Write-Host ([ProtoTest2]::Connect($host_ip))

# Try ALL methods from BOXPLAYER_DECOMPILATION.md plus variations
$methods = @(
    # HMGeneral
    "GetDeviceInfo","GetDeviceName","GetHardwareInfo","GetSDKTcpServer","GetAdminModeInfo",
    "GetScreenshot2","GetSystemVolume","GetDataSourceInfo","ReloadDeviceID",
    # HMProgram  
    "GetAllProgram","GetProgram","AddProgram","SwitchProgram","GetCurrentPlayProgramGUID",
    # HMHwSet
    "GetSDKFPGAConfig","GetBoxHwConfig","SmartSetting","SmartDrawLine",
    # HMLight
    "GetLuminancePloy",
    # HMScreenOnoff
    "GetSwitchTime","OpenScreen","CloseScreen",
    # HMTime
    "GetTimeInfo",
    # HMEthernet  
    "GetEth0Info","GetPppoeInfo","GetWifiInfo","GetNetworkInfo",
    # HMLicense
    "GetLicense",
    # HMUpgrade
    "FirmwareUpgrade","GetUpgradeResult","ExcuteUpgradeShell",
    # Other methods from NetIOServices.dll search
    "GetIFVersion","GetFirewareVersion","GetDeviceID","GetScreenInfo",
    "GetPlayStatus","GetDeviceLockerEnable","Reboot","RebootDevice",
    "GetBootLogo","GetFileChecklist","GetUpgradeSensorResult",
    "GetCustomResolution","GetLaunchApp","GetApps","GetWdtdValid",
    "GetIsShowIcon","GetRebootPloys","GetSensorInfo",
    # Case variations
    "getDeviceInfo","GET_DEVICE_INFO"
)

foreach ($m in $methods) {
    $result = [ProtoTest2]::TestMethod($m)
    Write-Host $result
}

[ProtoTest2]::Disconnect()
