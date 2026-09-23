

CALL=
# Use busybox-armv7l when it is RK Series
#CALL=busybox-armv7l
USER_CONFIG_FILE=/etc/passwd
PASSWD_CONFIG_FILE=/etc/shadow


# check if user exist
# $1 user
IsExistUser()
{
    if $CALL grep -E "^$1:" $USER_CONFIG_FILE > /dev/null ; then
        echo "1"
    else
        echo "0"
    fi
}

# check if user exist on password file
# $1 user
IsExistUserOnPasswd()
{
    if $CALL grep -E "^$1:" $PASSWD_CONFIG_FILE > /dev/null ; then
        echo "1"
    else
        echo "0"
    fi
}


# modify user password
# $1 user   $2 password
ModifyPassword()
{
    user=$1
    userPasswd=$2
    if [ -z "$user" ]; then
        echo "0"
        return 
    fi

    if [ -z "$userPasswd" ]; then
        echo "0"
        return 
    fi

    password="$userPasswd\n$userPasswd"
    echo -e "$password" | $CALL passwd "$user" > /dev/null 2>&1
    echo "1"
}


# Create user
CreateUser()
{
    user=$1
    uid=$2
    homePath=$3
    shPath=$4

    # a.sh user uid homePath shPath
    if [ -z "$user" ]; then
        echo "0"
        return 
    fi

    if [ -z "$uid" ]; then
        uid=0
    fi

    if [ -z "$homePath" ]; then
        if [ -d "/system/root" ]; then
            homePath="/system/root"
        elif [ -d "/root" ]; then
            homePath=/root
        else
            echo "0"
            return 
        fi
    fi

    if [ -z "$shPath" ]; then
        if [ -f "/system/bin/sh" ]; then
            shPath="/system/bin/sh"
        elif [ -f "/bin/sh" ]; then
            shPath="/bin/sh"
        else
            echo "0"
            return 
        fi
    fi

    result=$(IsExistUser "$user")
    if [ "$result" = "1" ]; then
        return 
    fi

    cmd="$user":x:"$uid":"$uid":"$user":"$homePath":"$shPath"
    echo "$cmd" >> $USER_CONFIG_FILE
    echo "1"
}


# PX30 create user
CreateUser_PX30()
{
    result=$(CreateUser "$@")
    if [ "$result" = "0" ]; then
        echo "0"
        return 
    fi

    user=$1
    result=$(IsExistUserOnPasswd "$user")
    if [ "$result" = "1" ]; then
        echo "1"
        return 
    fi

    if [ ! -f "$PASSWD_CONFIG_FILE" ]; then
        echo "2"
        return
    fi

    cmd="$user"':*:10933:0:99999:7:::'
    echo "$cmd" >> $PASSWD_CONFIG_FILE
    echo "1"
}


if [ "$1" = "IsExistUser" ]; then
    shift
    IsExistUser "$@"
elif [ "$1" = "IsExistUserOnPasswd" ]; then
    shift
    IsExistUserOnPasswd "$@"
elif [ "$1" = "ModifyPassword" ]; then
    shift
    ModifyPassword "$@"
elif [ "$1" = "CreateUser" ]; then
    shift
    CreateUser "$@"
elif [ "$1" = "CreateUser_PX30" ]; then
    shift
    CreateUser_PX30 "$@"
else
    echo "[ IsExistUser, IsExistUserOnPasswd, ModifyPassword, CreateUser, CreateUser_PX30 ]"
fi
