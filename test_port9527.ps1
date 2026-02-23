# Test what port 9527 TCP does: read first, then try various packets

Add-Type -TypeDefinition @"
using System;
using System.Net.Sockets;
using System.IO;
using System.Text;

public class PortTest {
    static byte[] ReadRaw(NetworkStream ns, int ms) {
        var acc = new System.Collections.Generic.List<byte>();
        var buf = new byte[65536]; ns.ReadTimeout = ms;
        var dl = DateTime.Now.AddMilliseconds(ms);
        while (DateTime.Now < dl) {
            try { int n = ns.Read(buf, 0, buf.Length); if(n==0) break; for(int i=0;i<n;i++) acc.Add(buf[i]); } catch(IOException){}
        }
        return acc.ToArray();
    }
    
    public static string Test(string host, int port, byte[] send_bytes) {
        try {
            var tcp = new TcpClient(); tcp.Connect(host, port);
            var ns = tcp.GetStream();
            byte[] pre = ReadRaw(ns, 300);
            string pre_hex = pre.Length > 0 ? "SERVER_FIRST: " + BitConverter.ToString(pre, 0, Math.Min(pre.Length, 40)) : "SERVER_SENDS_NOTHING_FIRST";
            if (send_bytes != null && send_bytes.Length > 0) {
                ns.Write(send_bytes, 0, send_bytes.Length);
                byte[] resp = ReadRaw(ns, 1000);
                string resp_hex = resp.Length > 0 ? "RESPONSE: " + BitConverter.ToString(resp, 0, Math.Min(resp.Length, 80)) : "NO_RESPONSE";
                tcp.Close();
                return pre_hex + " | " + resp_hex;
            }
            tcp.Close();
            return pre_hex;
        } catch(Exception ex) { return "ERROR: " + ex.Message; }
    }
    
    public static string TestRaw(string host, int port, byte[] send_bytes) {
        try {
            var tcp = new TcpClient(); tcp.Connect(host, port);
            var ns = tcp.GetStream();
            if (send_bytes != null) { ns.Write(send_bytes, 0, send_bytes.Length); }
            byte[] resp = ReadRaw(ns, 1500);
            bool closed = false;
            try { ns.ReadTimeout=100; int x = ns.ReadByte(); if(x==-1) closed=true; } catch {}
            tcp.Close();
            string info = resp.Length > 0 ? BitConverter.ToString(resp, 0, Math.Min(resp.Length, 80)) : "(empty)";
            return info + (closed ? " [CLOSED]" : " [OPEN]");
        } catch(Exception ex) { return "ERROR: " + ex.Message; }
    }
}
"@

$host_ip = "192.168.1.104"

Write-Host "=== Port 9527 TCP tests ==="

$boxStreamInit = [byte[]] @(0x08, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00)

Write-Host "Test 1: Does server send anything first? Then BoxStreamInit:"
Write-Host ([PortTest]::Test($host_ip, 9527, $boxStreamInit))

Start-Sleep -Milliseconds 200

Write-Host ""
Write-Host "Test 2: Send nothing -- does server close or keep open?"
Write-Host ([PortTest]::TestRaw($host_ip, 9527, $null))

Start-Sleep -Milliseconds 200

Write-Host ""
Write-Host "Test 3: SdkServiceAsk on port 9527 (old protocol):"
$sdkServiceAsk = [byte[]] @(0x08, 0x00, 0x01, 0x20, 0x00, 0x00, 0x00, 0x07)
Write-Host ([PortTest]::TestRaw($host_ip, 9527, $sdkServiceAsk))

Start-Sleep -Milliseconds 200

Write-Host ""
Write-Host "Test 4: HTTP GET on port 9527:"
$httpGet = [System.Text.Encoding]::ASCII.GetBytes("GET / HTTP/1.0`r`nHost: 192.168.1.104`r`n`r`n")
Write-Host ([PortTest]::TestRaw($host_ip, 9527, $httpGet))

Start-Sleep -Milliseconds 200

Write-Host ""
Write-Host "Test 5: What does port 10001 send first (before BoxStreamInit)?"
Write-Host ([PortTest]::Test($host_ip, 10001, $boxStreamInit))
