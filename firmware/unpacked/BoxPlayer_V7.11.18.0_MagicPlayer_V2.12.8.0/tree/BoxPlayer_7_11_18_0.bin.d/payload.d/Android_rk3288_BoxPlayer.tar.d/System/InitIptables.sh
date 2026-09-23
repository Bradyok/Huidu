

# disable telnet
iptables -C INPUT -p tcp --dport 23 -j DROP > /dev/null 2>&1
if [ "$?" = "1" ]; then
    iptables -A INPUT -p tcp --dport 23 -j DROP
fi
