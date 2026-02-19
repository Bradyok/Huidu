printf "ME3760  Pppoe\n"
killall cat
killall udhcpc
echo -e "AT+CFUN=1\r\n" >/dev/ttyUSB0
usleep 50000
echo -e "AT^SYSCONFIG=19,4,1,3\r\n" >/dev/ttyUSB0
usleep 25000
echo -e "AT+CEREG=1\r\n" >/dev/ttyUSB0
usleep 25000
echo -e "AT+COPS=1,2,\"46000\",0\r\n" >/dev/ttyUSB0
usleep 25000
echo -e "AT+CGDCONT=1,\"IP\"\r\n" >/dev/ttyUSB0
