echo -e "AT+CGACT=1,1\r\n" >/dev/ttyUSB0
usleep 25000
echo -e "AT+ZGACT=1,1\r\n" >/dev/ttyUSB0
