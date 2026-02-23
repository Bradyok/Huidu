# Test the exact BoxStream protocol sequence from Huidu.pcapng
# Sequence: HeartbeatAnswer(recv) -> HeartbeatAsk(send) -> BoxStreamInit(send) -> BoxStreamInitAck(recv) -> BoxStreamData+XML(send)

Add-Type -TypeDefinition @"
using System;
using System.Net.Sockets;
using System.Text;
using System.IO;

public class BoxStream {
    static TcpClient conn;
    static NetworkStream ns;
    
    static byte[] ReadExact(int ms) {
        var acc = new System.Collections.Generic.List<byte>();
        var buf = new byte[65536];
        ns.ReadTimeout = ms;
        var dl = DateTime.Now.AddMilliseconds(ms);
        while (DateTime.Now < dl) {
            try {
                int n = ns.Read(buf, 0, buf.Length);
                if (n == 0) { Console.WriteLine("[EOF] connection closed by device"); break; }
                for (int i = 0; i < n; i++) acc.Add(buf[i]);
                // Try to see if we have a complete packet
                if (acc.Count >= 4) {
                    int pktLen = (acc[0]) | (acc[1] << 8); // u16 LE
                    if (acc.Count >= pktLen) break; // got full packet
                }
            } catch(IOException) { break; }
        }
        return acc.ToArray();
    }
    
    static string Hex(byte[] b) { return b.Length > 0 ? BitConverter.ToString(b, 0, Math.Min(b.Length, 60)) : "(empty)"; }
    
    static byte[] MakePkt(ushort cmd, byte[] payload) {
        int total = 4 + payload.Length;
        byte[] pkt = new byte[total];
        pkt[0] = (byte)(total & 0xFF); pkt[1] = (byte)((total >> 8) & 0xFF);
        pkt[2] = (byte)(cmd & 0xFF); pkt[3] = (byte)((cmd >> 8) & 0xFF);
        Array.Copy(payload, 0, pkt, 4, payload.Length);
        return pkt;
    }
    
    public static string Run(string host) {
        try {
            conn = new TcpClient(); conn.Connect(host, 9527); ns = conn.GetStream();
            Console.WriteLine("Connected to " + host + ":9527");
            
            // Step 1: Wait for initial TcpHeartbeatAnswer (0x0060) from device
            Console.WriteLine("Waiting for initial heartbeat from device...");
            byte[] init = ReadExact(10000);
            Console.WriteLine("Received: " + Hex(init));
            if (init.Length < 4) return "ERROR: no initial packet from device";
            ushort initCmd = (ushort)(init[2] | (init[3] << 8));
            Console.WriteLine("Cmd: 0x" + initCmd.ToString("X4"));
            
            // Step 2: Respond with TcpHeartbeatAsk (0x005F)
            byte[] hbAsk = MakePkt(0x005F, new byte[0]);
            Console.WriteLine("Sending TcpHeartbeatAsk: " + Hex(hbAsk));
            ns.Write(hbAsk, 0, hbAsk.Length);
            
            System.Threading.Thread.Sleep(50);
            
            // Step 3: Send BoxStreamInit (0x0200) with payload [0,0,0,0]
            byte[] bsInit = MakePkt(0x0200, new byte[]{0,0,0,0});
            Console.WriteLine("Sending BoxStreamInit: " + Hex(bsInit));
            ns.Write(bsInit, 0, bsInit.Length);
            
            // Step 4: Wait for BoxStreamInitAck (0x0201)
            Console.WriteLine("Waiting for BoxStreamInitAck...");
            byte[] ack = ReadExact(5000);
            Console.WriteLine("Received: " + Hex(ack));
            if (ack.Length < 4) return "ERROR: no BoxStreamInitAck";
            ushort ackCmd = (ushort)(ack[2] | (ack[3] << 8));
            Console.WriteLine("Cmd: 0x" + ackCmd.ToString("X4"));
            if (ackCmd != 0x0201) return "ERROR: expected 0x0201, got 0x" + ackCmd.ToString("X4");
            
            // Step 5: Send BoxStreamData (0x0202) with [0x00,0x00] + XML batch request
            string xml = "<?xml version=\"1.0\" encoding=\"utf-8\"?><sdk guid=\"##GUID\"><in method=\"GetDeviceName\"/><in method=\"GetFirewareVersion\"/><in method=\"GetScreenInfo\"/></sdk>";
            byte[] xmlBytes = Encoding.UTF8.GetBytes(xml);
            byte[] payload = new byte[2 + xmlBytes.Length];
            payload[0] = 0; payload[1] = 0;  // direction: PC->device
            Array.Copy(xmlBytes, 0, payload, 2, xmlBytes.Length);
            byte[] bsData = MakePkt(0x0202, payload);
            Console.WriteLine("Sending BoxStreamData (" + bsData.Length + " bytes)");
            ns.Write(bsData, 0, bsData.Length);
            
            // Step 6: Wait for BoxStreamRxAck (0x0203)
            byte[] rxAck = ReadExact(3000);
            Console.WriteLine("BoxStreamRxAck: " + Hex(rxAck));
            ushort rxCmd = rxAck.Length >= 4 ? (ushort)(rxAck[2] | (rxAck[3] << 8)) : (ushort)0;
            Console.WriteLine("Cmd: 0x" + rxCmd.ToString("X4"));
            
            // Step 7: Send BoxStreamTxAck (0x0204)
            byte[] txAck = MakePkt(0x0204, new byte[]{0,0});
            Console.WriteLine("Sending BoxStreamTxAck: " + Hex(txAck));
            ns.Write(txAck, 0, txAck.Length);
            
            // Step 8: Wait for BoxStreamFinalAck (0x0205)
            byte[] finalAck = ReadExact(3000);
            Console.WriteLine("BoxStreamFinalAck: " + Hex(finalAck));
            ushort finalCmd = finalAck.Length >= 4 ? (ushort)(finalAck[2] | (finalAck[3] << 8)) : (ushort)0;
            Console.WriteLine("Cmd: 0x" + finalCmd.ToString("X4"));
            
            // Step 9: Wait for BoxStreamInit (0x0200) from device with payload [0x01,0x00,0x00,0x00] = response ready
            Console.WriteLine("Waiting for device response-ready signal...");
            byte[] respReady = ReadExact(10000);
            Console.WriteLine("Received: " + Hex(respReady));
            ushort rrCmd = respReady.Length >= 4 ? (ushort)(respReady[2] | (respReady[3] << 8)) : (ushort)0;
            Console.WriteLine("Cmd: 0x" + rrCmd.ToString("X4"));
            
            // Step 10: Send BoxStreamInitAck (0x0201) 
            byte[] initAck2 = MakePkt(0x0201, new byte[]{0,0});
            Console.WriteLine("Sending BoxStreamInitAck: " + Hex(initAck2));
            ns.Write(initAck2, 0, initAck2.Length);
            
            // Step 11: Wait for BoxStreamData response (0x0202)
            Console.WriteLine("Waiting for XML response...");
            // Read all response data (might be multiple reads for large response)
            var respBuf = new System.Collections.Generic.List<byte>();
            ns.ReadTimeout = 5000;
            var deadline = DateTime.Now.AddSeconds(8);
            while (DateTime.Now < deadline) {
                byte[] tmp = new byte[65536];
                try {
                    int n = ns.Read(tmp, 0, tmp.Length);
                    if (n == 0) { Console.WriteLine("[EOF]"); break; }
                    for (int i = 0; i < n; i++) respBuf.Add(tmp[i]);
                    // Check if we have the full packet
                    if (respBuf.Count >= 4) {
                        int expectedLen = respBuf[0] | (respBuf[1] << 8);
                        if (respBuf.Count >= expectedLen) break;
                    }
                } catch(IOException) { break; }
            }
            byte[] resp = respBuf.ToArray();
            Console.WriteLine("BoxStreamData response: " + resp.Length + " bytes");
            if (resp.Length >= 6) {
                ushort respCmd = (ushort)(resp[2] | (resp[3] << 8));
                Console.WriteLine("Cmd: 0x" + respCmd.ToString("X4"));
                if (resp.Length > 6) {
                    string xmlResp = Encoding.UTF8.GetString(resp, 6, Math.Min(resp.Length - 6, 800));
                    Console.WriteLine("XML: " + xmlResp.Substring(0, Math.Min(xmlResp.Length, 600)));
                }
            }
            
            conn.Close();
            return "DONE";
        } catch(Exception ex) {
            return "EXCEPTION: " + ex.Message;
        }
    }
}
"@

Write-Host ([BoxStream]::Run("192.168.1.104"))
