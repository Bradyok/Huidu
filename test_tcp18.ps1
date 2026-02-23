# Get full raw bytes of GetDeviceInfo response to understand data format

Add-Type -TypeDefinition @"
using System;
using System.Net.Sockets;
using System.Text;
using System.IO;
using System.Text.RegularExpressions;

public class Proto18 {
    static byte[] MakePkt(ushort cmd, byte[] payload) {
        int total = 4 + payload.Length;
        byte[] pkt = new byte[total]; pkt[0]=(byte)(total&0xFF); pkt[1]=(byte)((total>>8)&0xFF);
        pkt[2]=(byte)(cmd&0xFF); pkt[3]=(byte)((cmd>>8)&0xFF); Array.Copy(payload, 0, pkt, 4, payload.Length); return pkt;
    }
    static byte[] ReadRaw(NetworkStream ns, int ms) {
        var acc = new System.Collections.Generic.List<byte>(); var buf = new byte[65536]; ns.ReadTimeout = 500;
        var dl = DateTime.Now.AddMilliseconds(ms);
        while (DateTime.Now < dl) { try { int n = ns.Read(buf, 0, buf.Length); if(n==0) break; for(int i=0;i<n;i++) acc.Add(buf[i]); } catch(IOException){} }
        return acc.ToArray();
    }
    static TcpClient conn; static NetworkStream ns_global; static string sessionGuid = "INITGUID";
    public static string Connect(string host) {
        conn = new TcpClient(); conn.Connect(host, 10001); ns_global = conn.GetStream();
        ns_global.Write(MakePkt(0x2001, new byte[]{0,0,0,7}), 0, 8); ReadRaw(ns_global, 500);
        // Register session with GetIFVersion
        string xml = "<?xml version=\"1.0\" encoding=\"utf-8\"?><sdk guid=\""+sessionGuid+"\"><in method=\"GetIFVersion\"><version value=\"1000000\"/></in></sdk>";
        byte[] xb = Encoding.UTF8.GetBytes(xml); byte[] f = new byte[8+xb.Length];
        f[0]=(byte)(xb.Length&0xFF); f[1]=(byte)((xb.Length>>8)&0xFF); Array.Copy(xb,0,f,8,xb.Length);
        ns_global.Write(MakePkt(0x2003, f), 0, MakePkt(0x2003, f).Length);
        byte[] r = ReadRaw(ns_global, 1000);
        if (r.Length > 12) {
            string resp_xml = Encoding.UTF8.GetString(r, 12, Math.Min(r.Length-12, 400));
            var m = Regex.Match(resp_xml, @"<sdk guid=""([^""]+)""");
            if (m.Success) { sessionGuid = m.Groups[1].Value; return "Registered, session GUID: " + sessionGuid; }
        }
        return "Failed to register";
    }
    public static void Disconnect() { if(conn!=null){try{conn.Close();}catch{}}}
    public static void CallMethodFull(string method, string body="") {
        string xml = "<?xml version=\"1.0\" encoding=\"utf-8\"?><sdk guid=\""+sessionGuid+"\"><in method=\""+method+"\">"+body+"</in></sdk>";
        byte[] xb = Encoding.UTF8.GetBytes(xml); byte[] f = new byte[8+xb.Length];
        f[0]=(byte)(xb.Length&0xFF); f[1]=(byte)((xb.Length>>8)&0xFF); Array.Copy(xb,0,f,8,xb.Length);
        ns_global.Write(MakePkt(0x2003, f), 0, MakePkt(0x2003, f).Length);
        byte[] resp = ReadRaw(ns_global, 5000);
        Console.WriteLine("Method: " + method);
        Console.WriteLine("  Total bytes: " + resp.Length);
        if (resp.Length >= 4) {
            ushort cmd = (ushort)(resp[2]|(resp[3]<<8));
            Console.WriteLine("  cmd: 0x" + cmd.ToString("X4"));
            if (resp.Length >= 8) {
                int xmlLen = resp[4]|(resp[5]<<8)|(resp[6]<<16)|(resp[7]<<24);
                Console.WriteLine("  xml_len_field: " + xmlLen);
            }
            if (resp.Length >= 12) {
                int chunkIdx = resp[8]|(resp[9]<<8)|(resp[10]<<16)|(resp[11]<<24);
                Console.WriteLine("  chunk_idx: " + chunkIdx);
            }
            if (resp.Length > 12) {
                string xmlStr = Encoding.UTF8.GetString(resp, 12, resp.Length-12);
                Console.WriteLine("  XML: " + xmlStr.Substring(0, Math.Min(xmlStr.Length, 600)));
            }
        }
        Console.WriteLine("  Raw hex: " + BitConverter.ToString(resp, 0, Math.Min(resp.Length, 80)));
    }
}
"@

$host_ip = "192.168.1.104"
Write-Host ([Proto18]::Connect($host_ip))
Write-Host ""

# Test methods that should have data, show full raw bytes
[Proto18]::CallMethodFull("GetDeviceInfo")
Write-Host ""
[Proto18]::CallMethodFull("GetTimeInfo")
Write-Host ""
[Proto18]::CallMethodFull("GetEth0Info")
Write-Host ""
[Proto18]::CallMethodFull("GetAllProgram")
Write-Host ""
[Proto18]::CallMethodFull("GetLuminancePloy")
Write-Host ""
[Proto18]::CallMethodFull("GetDeviceName")

[Proto18]::Disconnect()
