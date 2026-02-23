Add-Type -TypeDefinition @"
using System;
using System.Net.Sockets;
using System.Text;
using System.IO;

public class BoxTest2 {
    static byte[] ReadFor(NetworkStream ns, int ms) {
        var acc = new System.Collections.Generic.List<byte>();
        var buf = new byte[65536]; ns.ReadTimeout = ms;
        var dl = DateTime.Now.AddMilliseconds(ms);
        while (DateTime.Now < dl) {
            try { int n = ns.Read(buf,0,buf.Length); if(n==0){Console.WriteLine("[EOF]");break;} for(int i=0;i<n;i++) acc.Add(buf[i]); } catch(IOException){break;}
        }
        return acc.ToArray();
    }
    static byte[] Pkt(ushort cmd, byte[] pay) {
        int t=4+pay.Length; var b=new byte[t];
        b[0]=(byte)(t&0xFF);b[1]=(byte)((t>>8)&0xFF);
        b[2]=(byte)(cmd&0xFF);b[3]=(byte)((cmd>>8)&0xFF);
        Array.Copy(pay,0,b,4,pay.Length); return b;
    }
    static string Hex(byte[] b, int max=40){ return b.Length>0?BitConverter.ToString(b,0,Math.Min(b.Length,max)):"(empty)"; }
    
    static bool DoHandshake(TcpClient tcp) {
        var ns = tcp.GetStream();
        byte[] hb = ReadFor(ns, 8000);
        Console.WriteLine("  Initial: " + Hex(hb));
        if (hb.Length < 4) return false;
        ushort cmd = (ushort)(hb[2]|(hb[3]<<8));
        if (cmd == 0x0060) {
            Console.WriteLine("  Got TcpHeartbeatAnswer, sending TcpHeartbeatAsk");
            ns.Write(Pkt(0x005F, new byte[0]), 0, 4);
        } else {
            Console.WriteLine("  Got unexpected cmd: 0x" + cmd.ToString("X4"));
        }
        return true;
    }
    
    public static void TestBoxStreamInit(string host, byte[] initPayload, string desc) {
        Console.WriteLine("");
        Console.WriteLine("=== BoxStreamInit test: " + desc + " ===");
        try {
            var tcp = new TcpClient(); tcp.Connect(host, 9527);
            var ns = tcp.GetStream();
            DoHandshake(tcp);
            System.Threading.Thread.Sleep(100);
            byte[] pkt = Pkt(0x0200, initPayload);
            Console.WriteLine("  Sending: " + Hex(pkt));
            ns.Write(pkt, 0, pkt.Length);
            byte[] resp = ReadFor(ns, 3000);
            Console.WriteLine("  Response: " + Hex(resp));
            if (resp.Length >= 4) { ushort c=(ushort)(resp[2]|(resp[3]<<8)); Console.WriteLine("  Cmd: 0x"+c.ToString("X4")); }
            tcp.Close();
        } catch(Exception ex) { Console.WriteLine("  ERROR: " + ex.Message); }
    }
    
    public static void TestDirectData(string host) {
        Console.WriteLine("");
        Console.WriteLine("=== Try BoxStreamData directly (no BoxStreamInit) ===");
        try {
            var tcp = new TcpClient(); tcp.Connect(host, 9527);
            var ns = tcp.GetStream();
            DoHandshake(tcp);
            System.Threading.Thread.Sleep(100);
            string xml = "<?xml version=\"1.0\" encoding=\"utf-8\"?><sdk guid=\"##GUID\"><in method=\"GetDeviceName\"/></sdk>";
            byte[] xb = Encoding.UTF8.GetBytes(xml);
            byte[] pay = new byte[2+xb.Length]; pay[0]=0; pay[1]=0; Array.Copy(xb,0,pay,2,xb.Length);
            byte[] pkt = Pkt(0x0202, pay);
            Console.WriteLine("  Sending BoxStreamData (" + pkt.Length + " bytes)");
            ns.Write(pkt, 0, pkt.Length);
            byte[] resp = ReadFor(ns, 3000);
            Console.WriteLine("  Response: " + Hex(resp));
            if (resp.Length >= 4) { ushort c=(ushort)(resp[2]|(resp[3]<<8)); Console.WriteLine("  Cmd: 0x"+c.ToString("X4")); }
            tcp.Close();
        } catch(Exception ex) { Console.WriteLine("  ERROR: " + ex.Message); }
    }
    
    public static void TestKeepalive(string host) {
        Console.WriteLine("");
        Console.WriteLine("=== Stay connected and wait for device to send more data ===");
        try {
            var tcp = new TcpClient(); tcp.Connect(host, 9527);
            var ns = tcp.GetStream();
            DoHandshake(tcp);
            // Don't send BoxStreamInit -- just listen for 15 seconds
            Console.WriteLine("  Listening for 15 seconds without sending BoxStreamInit...");
            var buf = ReadFor(ns, 15000);
            Console.WriteLine("  Received " + buf.Length + " bytes: " + Hex(buf));
            tcp.Close();
        } catch(Exception ex) { Console.WriteLine("  ERROR: " + ex.Message); }
    }
}
"@

$host_ip = "192.168.1.104"

# Try different BoxStreamInit payloads
[BoxTest2]::TestBoxStreamInit($host_ip, [byte[]]@(0,0,0,0), "payload=00000000 (pcap value)")
Start-Sleep -Milliseconds 500
[BoxTest2]::TestBoxStreamInit($host_ip, [byte[]]@(1,0,0,0), "payload=01000000")
Start-Sleep -Milliseconds 500
[BoxTest2]::TestBoxStreamInit($host_ip, [byte[]]@(7,0,0,0), "payload=07000000 (client version)")
Start-Sleep -Milliseconds 500
[BoxTest2]::TestBoxStreamInit($host_ip, [byte[]]@(0,0,0,0,0,0,0,0), "payload=8 zeros")
Start-Sleep -Milliseconds 500

# Try sending BoxStreamData directly
[BoxTest2]::TestDirectData($host_ip)
Start-Sleep -Milliseconds 500

# Stay connected and listen
[BoxTest2]::TestKeepalive($host_ip)
