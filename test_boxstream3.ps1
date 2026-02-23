# Test hypothesis: does port 10001 authentication "unlock" port 9527 BoxStream?
# Also test: BoxStreamInit with NO heartbeat response, and with explicit wait for second heartbeat

Add-Type -TypeDefinition @"
using System;
using System.Net.Sockets;
using System.Text;
using System.IO;
using System.Text.RegularExpressions;

public class BS3 {
    static byte[] Pkt(ushort cmd, byte[] pay) {
        int t=4+pay.Length; var b=new byte[t];
        b[0]=(byte)(t&0xFF);b[1]=(byte)((t>>8)&0xFF);
        b[2]=(byte)(cmd&0xFF);b[3]=(byte)((cmd>>8)&0xFF);
        Array.Copy(pay,0,b,4,pay.Length); return b;
    }
    static string Hex(byte[] b, int max=60){ return b.Length>0?BitConverter.ToString(b,0,Math.Min(b.Length,max)):"(empty)"; }
    static byte[] ReadFor(NetworkStream ns, int ms, int minBytes=4) {
        var acc = new System.Collections.Generic.List<byte>();
        ns.ReadTimeout = ms;
        var dl = DateTime.Now.AddMilliseconds(ms);
        var buf = new byte[65536];
        while (DateTime.Now < dl) {
            try {
                int n = ns.Read(buf, 0, buf.Length);
                if (n == 0) break;
                for(int i=0;i<n;i++) acc.Add(buf[i]);
                if (acc.Count >= minBytes) break;
            } catch(IOException) { break; }
        }
        return acc.ToArray();
    }

    public static string Auth10001(string host) {
        try {
            var tcp = new TcpClient(); tcp.Connect(host, 10001);
            var ns = tcp.GetStream();
            byte[] ask = Pkt(0x2001, new byte[]{0,0,0,7});
            ns.Write(ask, 0, ask.Length);
            byte[] resp = ReadFor(ns, 3000, 6);
            ushort cmd = resp.Length >= 4 ? (ushort)(resp[2]|(resp[3]<<8)) : (ushort)0;
            Console.WriteLine("Port 10001 SdkServiceAnswer: cmd=0x" + cmd.ToString("X4") + " resp=" + Hex(resp));
            if (cmd != 0x2002) { tcp.Close(); return "FAIL: no SdkServiceAnswer"; }
            string guid = "TESTGUID-0001-0002-0003-000000000001";
            string xml = "<?xml version=\"1.0\" encoding=\"utf-8\"?><sdk guid=\"" + guid + "\"><in method=\"GetIFVersion\"><version value=\"1000000\"/></in></sdk>";
            byte[] xb = Encoding.UTF8.GetBytes(xml);
            byte[] frame = new byte[8 + xb.Length];
            frame[0] = (byte)(xb.Length & 0xFF); frame[1] = (byte)((xb.Length >> 8) & 0xFF);
            Array.Copy(xb, 0, frame, 8, xb.Length);
            ns.Write(Pkt(0x2003, frame), 0, 4 + 8 + xb.Length);
            byte[] resp2 = ReadFor(ns, 3000, 20);
            Console.WriteLine("Port 10001 GetIFVersion response: " + resp2.Length + " bytes: " + Hex(resp2));
            if (resp2.Length > 12) {
                string rxstr = Encoding.UTF8.GetString(resp2, 12, Math.Min(resp2.Length-12, 200));
                Console.WriteLine("  XML: " + rxstr.Substring(0, Math.Min(rxstr.Length, 200)));
                var m = Regex.Match(rxstr, "sdk guid=\"([^\"]+)\"");
                if (m.Success) { Console.WriteLine("  Session GUID: " + m.Groups[1].Value); }
            }
            tcp.Close();
            return "OK";
        } catch(Exception ex) { return "ERROR: " + ex.Message; }
    }

    public static void TestInitBeforeHeartbeat(string host) {
        Console.WriteLine("");
        Console.WriteLine("=== BoxStreamInit BEFORE heartbeat response ===");
        try {
            var tcp = new TcpClient(); tcp.Connect(host, 9527);
            var ns = tcp.GetStream();
            byte[] bsi = Pkt(0x0200, new byte[]{0,0,0,0});
            Console.WriteLine("  Sending BoxStreamInit immediately: " + Hex(bsi));
            ns.Write(bsi, 0, bsi.Length);
            byte[] resp = ReadFor(ns, 3000, 4);
            Console.WriteLine("  Response: " + Hex(resp));
            if (resp.Length >= 4) { Console.WriteLine("  Cmd: 0x" + ((ushort)(resp[2]|(resp[3]<<8))).ToString("X4")); }
            tcp.Close();
        } catch(Exception ex) { Console.WriteLine("  ERROR: " + ex.Message); }
    }

    public static void TestRespondAllHeartbeats(string host) {
        Console.WriteLine("");
        Console.WriteLine("=== Respond to ALL heartbeats then BoxStreamInit ===");
        try {
            var tcp = new TcpClient(); tcp.Connect(host, 9527);
            var ns = tcp.GetStream();
            Console.WriteLine("  Collecting all initial heartbeats for 2s...");
            byte[] init = ReadFor(ns, 2000, 999999);
            Console.WriteLine("  Got " + init.Length + " bytes: " + Hex(init));
            int count = 0;
            for (int i = 0; i + 3 < init.Length; i += 4) {
                if (init[i]==4 && init[i+1]==0 && init[i+2]==0x60 && init[i+3]==0) count++;
            }
            Console.WriteLine("  Got " + count + " heartbeats, responding to each...");
            for (int i = 0; i < count; i++) {
                ns.Write(Pkt(0x005F, new byte[0]), 0, 4);
            }
            System.Threading.Thread.Sleep(200);
            byte[] bsi = Pkt(0x0200, new byte[]{0,0,0,0});
            Console.WriteLine("  Sending BoxStreamInit: " + Hex(bsi));
            ns.Write(bsi, 0, bsi.Length);
            byte[] resp = ReadFor(ns, 3000, 4);
            Console.WriteLine("  Response: " + Hex(resp));
            if (resp.Length >= 4) { Console.WriteLine("  Cmd: 0x" + ((ushort)(resp[2]|(resp[3]<<8))).ToString("X4")); }
            tcp.Close();
        } catch(Exception ex) { Console.WriteLine("  ERROR: " + ex.Message); }
    }
    
    public static void TestAuthThenBoxStream(string host) {
        Console.WriteLine("");
        Console.WriteLine("=== Port 10001 auth then port 9527 BoxStreamInit ===");
        string authResult = Auth10001(host);
        Console.WriteLine("Auth result: " + authResult);
        System.Threading.Thread.Sleep(300);
        try {
            var tcp = new TcpClient(); tcp.Connect(host, 9527);
            var ns = tcp.GetStream();
            byte[] init = ReadFor(ns, 3000, 4);
            Console.WriteLine("  Initial: " + Hex(init));
            int i = 0;
            while (i + 3 < init.Length) {
                if (init[i]==4 && init[i+1]==0 && init[i+2]==0x60 && init[i+3]==0) {
                    ns.Write(Pkt(0x005F, new byte[0]), 0, 4);
                }
                i += 4;
            }
            System.Threading.Thread.Sleep(100);
            byte[] bsi = Pkt(0x0200, new byte[]{0,0,0,0});
            Console.WriteLine("  Sending BoxStreamInit: " + Hex(bsi));
            ns.Write(bsi, 0, bsi.Length);
            byte[] resp = ReadFor(ns, 3000, 4);
            Console.WriteLine("  Response: " + Hex(resp));
            if (resp.Length >= 4) { Console.WriteLine("  Cmd: 0x" + ((ushort)(resp[2]|(resp[3]<<8))).ToString("X4")); }
            tcp.Close();
        } catch(Exception ex) { Console.WriteLine("  ERROR: " + ex.Message); }
    }
    
    public static void TestExactPcapXml(string host) {
        Console.WriteLine("");
        Console.WriteLine("=== Full BoxStream session with EXACT pcap XML (25 methods) ===");
        try {
            var tcp = new TcpClient(); tcp.Connect(host, 9527);
            var ns = tcp.GetStream();
            byte[] init = ReadFor(ns, 3000, 4);
            Console.WriteLine("  Initial: " + Hex(init));
            int i = 0;
            while (i + 3 < init.Length) {
                if (init[i]==4 && init[i+1]==0 && init[i+2]==0x60 && init[i+3]==0)
                    ns.Write(Pkt(0x005F, new byte[0]), 0, 4);
                i += 4;
            }
            System.Threading.Thread.Sleep(100);
            ns.Write(Pkt(0x0200, new byte[]{0,0,0,0}), 0, 8);
            byte[] ack = ReadFor(ns, 3000, 4);
            Console.WriteLine("  BoxStreamInitAck: " + Hex(ack));
            if (ack.Length < 4) { tcp.Close(); return; }
            string xml = "<?xml version=\"1.0\" encoding=\"utf-8\"?><sdk guid=\"##GUID\"><in method=\"GetDeviceName\"/><in method=\"GetFirewareVersion\"/><in method=\"GetKeyDefine\"/><in method=\"GetPlayStatus\"/><in method=\"GetSystemVolume\"/><in method=\"GetBootLogo\"/><in method=\"GetSensorInfo\"/><in method=\"GetGPSInfo\"/><in method=\"GetCurrentLuminance\"/><in method=\"GetCurrentTemperature\"/><in method=\"GetCurrentHumity\"/><in method=\"GetSensorType\"/><in method=\"GetSwitchTime\"/><in method=\"GetTimeInfo\"/><in method=\"GetLuminancePloy\"/><in method=\"GetScreenInfo\"/><in method=\"GetLicense\"/><in method=\"GetEth0Info\"/><in method=\"GetWifiInfo\"/><in method=\"GetPppoeInfo\"/><in method=\"GetDeviceInfo\"/><in method=\"GetDataSourceInfo\"/><in method=\"GetRelay\"/></sdk>";
            byte[] xb = Encoding.UTF8.GetBytes(xml);
            byte[] pay = new byte[2+xb.Length]; Array.Copy(xb,0,pay,2,xb.Length);
            ns.Write(Pkt(0x0202, pay), 0, 4+2+xb.Length);
            byte[] rxAck = ReadFor(ns, 3000, 4); Console.WriteLine("  RxAck: " + Hex(rxAck));
            ns.Write(Pkt(0x0204, new byte[]{0,0}), 0, 6);
            byte[] fAck = ReadFor(ns, 3000, 4); Console.WriteLine("  FinalAck: " + Hex(fAck));
            byte[] ready = ReadFor(ns, 10000, 4); Console.WriteLine("  Ready: " + Hex(ready));
            tcp.Close();
        } catch(Exception ex) { Console.WriteLine("  ERROR: " + ex.Message); }
    }
}
"@

$host_ip = "192.168.1.104"

[BS3]::TestInitBeforeHeartbeat($host_ip)
Start-Sleep -Milliseconds 1000

[BS3]::TestRespondAllHeartbeats($host_ip)
Start-Sleep -Milliseconds 1000

[BS3]::TestAuthThenBoxStream($host_ip)
Start-Sleep -Milliseconds 1000

[BS3]::TestExactPcapXml($host_ip)
