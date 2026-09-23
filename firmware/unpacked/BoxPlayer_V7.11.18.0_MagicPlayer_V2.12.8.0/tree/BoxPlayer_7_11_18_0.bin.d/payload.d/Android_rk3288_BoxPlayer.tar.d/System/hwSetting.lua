--[[
lua版本：5.1.4
作者：雷念
日期：2018/6/29

>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>
版本:V1.0
描述：支持修改亮度，修改发送卡网口1，网口2带载范围，坐标。支持连接线
>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>

]]---


-->>>>>>>>>>>参数定义段
local A1 = 0
local B1 = 16 * 1
local C1 = 16 * 2
local D1 = 16 * 3
local E1 = 16 * 4
local F1 = 16 * 5
local G1 = 16 * 6
local H1 = 16 * 7
local J1 = 16 * 8
local K1 = 16 * 9
local L1 = 16 * 10
local M1 = 16 * 11
local N1 = 16 * 12
local P1 = 16 * 13
local R1 = 16 * 14
local T1 = 16 * 15
local AA1 = 16 * 16
local AB1 = 16 * 17
local AC1 = 16 * 18
local AD1 = 16 * 19
local AE1 = 16 * 20
local AF1 = 16 * 21
local AG1 = 16 * 22
local AH1 = 16 * 23
local AJ1 = 16 * 24
local AK1 = 16 * 25
local AL1 = 16 * 26
local AM1 = 16 * 27
local AN1 = 16 * 28
local AP1 = 16 * 29
local AR1 = 16 * 30
local AT1 = 16 * 31



local HEAD_LENGTH            = 8 ----头长度
local CONTROL_LENGTH         = 9--控制段头长度
local CRC_LENGTH             = 4--CRC长度
local EFFECTIVE_DATA_LENGTH  = 512--数据长度

local TOTAL_LENGTH           = HEAD_LENGTH+CONTROL_LENGTH+EFFECTIVE_DATA_LENGTH+CRC_LENGTH -- 总长度

local DATA_TYPE_CONTROL  =  0x01--//控制段
local DATA_TYPE_SEND     =  0x02--//发送参数段
local DATA_TYPE_FEEDBACK =  0x03--//反馈参数段

local SENDING_CARD_PARAMETERS  = 0X0000 -- 发送卡参数
local PROBO_RECEIVER_CARD      = 0x0001 -- 探测接收卡
local PROBO_CONNECT            = 0x0005 -- 连接关系
local PROBO_CONNECT_NEW        = 0x0015 -- 连接关系新架构

local CellWidth = 0
local CellHight = 0

local A601 = 0--设备类型
local A602 = 1
local A603 = 2
local DT_D1 = 6
local DT_D3 = 3
local DT_C1i = 4
local DT_C3i = 5
local DT_C10 = 13
local DT_C30 = 7
local DT_D30 = 11
local DT_D10 = 12
local DT_D20 = 14
local DT_A30 = 8
local DT_A10Plus = 9
local DT_A30Plus = 10
local DT_C30Plus = 15
local DT_C10Plus = 16
local DT_V10 = 17
local DT_A3 = 18
local DT_A3Plus = 19
local DT_V02 = 20
local DT_A6  = 21
local DT_D15  = 30
local DT_D35  = 31
local DT_C15  = 32
local DT_C35  = 33
local DT_C15C  = 34
local DT_A3N  = 35
local DT_A4  = 36
local DT_A5  = 37
local DT_B6  = 42
local DT_C16 = 46
local DT_C36 = 47
local DT_D16  = 48
local DT_D36  = 49
local DT_D68  = 44
local DT_D18  = 51
-->>>>>>>>>>>end>>>>

--[[
函数：crc32(array)
备注：计算CRC
返回：返回计算后的值
]]---
local tab = {
            0x00000000, 0x77073096, 0xee0e612c, 0x990951ba,
            0x076dc419, 0x706af48f, 0xe963a535, 0x9e6495a3,
            0x0edb8832, 0x79dcb8a4, 0xe0d5e91e, 0x97d2d988,
            0x09b64c2b, 0x7eb17cbd, 0xe7b82d07, 0x90bf1d91,
            0x1db71064, 0x6ab020f2, 0xf3b97148, 0x84be41de,
            0x1adad47d, 0x6ddde4eb, 0xf4d4b551, 0x83d385c7,
            0x136c9856, 0x646ba8c0, 0xfd62f97a, 0x8a65c9ec,
            0x14015c4f, 0x63066cd9, 0xfa0f3d63, 0x8d080df5,
            0x3b6e20c8, 0x4c69105e, 0xd56041e4, 0xa2677172,
            0x3c03e4d1, 0x4b04d447, 0xd20d85fd, 0xa50ab56b,
            0x35b5a8fa, 0x42b2986c, 0xdbbbc9d6, 0xacbcf940,
            0x32d86ce3, 0x45df5c75, 0xdcd60dcf, 0xabd13d59,
            0x26d930ac, 0x51de003a, 0xc8d75180, 0xbfd06116,
            0x21b4f4b5, 0x56b3c423, 0xcfba9599, 0xb8bda50f,
            0x2802b89e, 0x5f058808, 0xc60cd9b2, 0xb10be924,
            0x2f6f7c87, 0x58684c11, 0xc1611dab, 0xb6662d3d,
            0x76dc4190, 0x01db7106, 0x98d220bc, 0xefd5102a,
            0x71b18589, 0x06b6b51f, 0x9fbfe4a5, 0xe8b8d433,
            0x7807c9a2, 0x0f00f934, 0x9609a88e, 0xe10e9818,
            0x7f6a0dbb, 0x086d3d2d, 0x91646c97, 0xe6635c01,
            0x6b6b51f4, 0x1c6c6162, 0x856530d8, 0xf262004e,
            0x6c0695ed, 0x1b01a57b, 0x8208f4c1, 0xf50fc457,
            0x65b0d9c6, 0x12b7e950, 0x8bbeb8ea, 0xfcb9887c,
            0x62dd1ddf, 0x15da2d49, 0x8cd37cf3, 0xfbd44c65,
            0x4db26158, 0x3ab551ce, 0xa3bc0074, 0xd4bb30e2,
            0x4adfa541, 0x3dd895d7, 0xa4d1c46d, 0xd3d6f4fb,
            0x4369e96a, 0x346ed9fc, 0xad678846, 0xda60b8d0,
            0x44042d73, 0x33031de5, 0xaa0a4c5f, 0xdd0d7cc9,
            0x5005713c, 0x270241aa, 0xbe0b1010, 0xc90c2086,
            0x5768b525, 0x206f85b3, 0xb966d409, 0xce61e49f,
            0x5edef90e, 0x29d9c998, 0xb0d09822, 0xc7d7a8b4,
            0x59b33d17, 0x2eb40d81, 0xb7bd5c3b, 0xc0ba6cad,
            0xedb88320, 0x9abfb3b6, 0x03b6e20c, 0x74b1d29a,
            0xead54739, 0x9dd277af, 0x04db2615, 0x73dc1683,
            0xe3630b12, 0x94643b84, 0x0d6d6a3e, 0x7a6a5aa8,
            0xe40ecf0b, 0x9309ff9d, 0x0a00ae27, 0x7d079eb1,
            0xf00f9344, 0x8708a3d2, 0x1e01f268, 0x6906c2fe,
            0xf762575d, 0x806567cb, 0x196c3671, 0x6e6b06e7,
            0xfed41b76, 0x89d32be0, 0x10da7a5a, 0x67dd4acc,
            0xf9b9df6f, 0x8ebeeff9, 0x17b7be43, 0x60b08ed5,
            0xd6d6a3e8, 0xa1d1937e, 0x38d8c2c4, 0x4fdff252,
            0xd1bb67f1, 0xa6bc5767, 0x3fb506dd, 0x48b2364b,
            0xd80d2bda, 0xaf0a1b4c, 0x36034af6, 0x41047a60,
            0xdf60efc3, 0xa867df55, 0x316e8eef, 0x4669be79,
            0xcb61b38c, 0xbc66831a, 0x256fd2a0, 0x5268e236,
            0xcc0c7795, 0xbb0b4703, 0x220216b9, 0x5505262f,
            0xc5ba3bbe, 0xb2bd0b28, 0x2bb45a92, 0x5cb36a04,
            0xc2d7ffa7, 0xb5d0cf31, 0x2cd99e8b, 0x5bdeae1d,
            0x9b64c2b0, 0xec63f226, 0x756aa39c, 0x026d930a,
            0x9c0906a9, 0xeb0e363f, 0x72076785, 0x05005713,
            0x95bf4a82, 0xe2b87a14, 0x7bb12bae, 0x0cb61b38,
            0x92d28e9b, 0xe5d5be0d, 0x7cdcefb7, 0x0bdbdf21,
            0x86d3d2d4, 0xf1d4e242, 0x68ddb3f8, 0x1fda836e,
            0x81be16cd, 0xf6b9265b, 0x6fb077e1, 0x18b74777,
            0x88085ae6, 0xff0f6a70, 0x66063bca, 0x11010b5c,
            0x8f659eff, 0xf862ae69, 0x616bffd3, 0x166ccf45,
            0xa00ae278, 0xd70dd2ee, 0x4e048354, 0x3903b3c2,
            0xa7672661, 0xd06016f7, 0x4969474d, 0x3e6e77db,
            0xaed16a4a, 0xd9d65adc, 0x40df0b66, 0x37d83bf0,
            0xa9bcae53, 0xdebb9ec5, 0x47b2cf7f, 0x30b5ffe9,
            0xbdbdf21c, 0xcabac28a, 0x53b39330, 0x24b4a3a6,
            0xbad03605, 0xcdd70693, 0x54de5729, 0x23d967bf,
            0xb3667a2e, 0xc4614ab8, 0x5d681b02, 0x2a6f2b94,
            0xb40bbe37, 0xc30c8ea1, 0x5a05df1b, 0x2d02ef8d
}
local function xor(a, b)
    local calc = 0

    for i = 32, 0, -1 do
    local val = 2 ^ i
    local aa = false
    local bb = false

    if a == 0 then
        calc = calc + b
        break
    end

    if b == 0 then
        calc = calc + a
        break
    end

    if a >= val then
        aa = true
        a = a - val
    end

    if b >= val then
        bb = true
        b = b - val
    end

    if not (aa and bb) and (aa or bb) then
        calc = calc + val
    end
    end

    return math.floor(calc)
end
-->>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>
ZZMathBit = {}
function ZZMathBit.__base(left, right, op) --对每一位进行op运算，然后将值返回
    if left < right then
        left, right = right, left
    end
    local res = 0
    local ScreenHeightift = 1
    while left ~= 0 do
        local ra = left % 2    --取得每一位(最右边)
        local rb = right % 2
        res = ScreenHeightift * op(ra,rb) + res
        ScreenHeightift = ScreenHeightift * 2
        left = math.modf( left / 2)  --右移
        right = math.modf( right / 2)
    end
    return res
end
-->>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>
function ZZMathBit.__andBit(left,right)    --与
    return (left == 1 and right == 1) and 1 or 0
end
-->>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>
function ZZMathBit.__orBit(left, right)    --或
    return (left == 1 or right == 1) and 1 or 0
end

function leftBit(leftdata, bit)    --右移
    for i=1,bit do
       leftdata = math.modf(leftdata  / 2)
    end
    return leftdata
end
-->>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>

function ZZMathBit.orOp(left, right)
    return ZZMathBit.__base(left, right, ZZMathBit.__orBit)
end
-->>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>
function ZZMathBit.andOp(left, right)
    return ZZMathBit.__base(left, right, ZZMathBit.__andBit)
end

local function lScreenHeightift(num, left)
    local res = num * (2 ^ left)
    return res % (2 ^ 32)
end
-->>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>
local function rScreenHeightift(num, right)
    local data=(2 ^ right)
	if data == 0
	then
	    data = 1;
	end
    local res = num / data
    return math.floor(res)
end
-->>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>
local CRC32 = {}

function CRC32.haScreenHeight(array,start_index,end_index)
    if end_index < start_index then return false end

    local crc = 0xFFFFFFFF
    for i=start_index,end_index do
        local byte = array[i]
        local a = ZZMathBit.andOp(rScreenHeightift(crc,8) , 0x00FFFFFF)
        local b = tab[ZZMathBit.andOp(xor(crc,byte) , 0xFF)+1]
        crc =  xor(a , b)
    end
    crc = xor(crc , 0xFFFFFFFF)
    crc = ZZMathBit.orOp(crc , - ZZMathBit.andOp(crc , lScreenHeightift(1,31)))
    crc = ZZMathBit.andOp(crc , 0xFFFFFFFF)

    return crc
end
-->>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>
--[[
函数：toInt(array,start_index)
备注：在数组中以start_index开始4个字节转换成数字
返回：返回计算后的值
]]---
local function toInt(array,index)
    local crc_bayte = 0
    crc_bayte = crc_bayte + array[index + 3]
    crc_bayte = crc_bayte * 256
    crc_bayte = crc_bayte + array[index + 2]
    crc_bayte = crc_bayte * 256
    crc_bayte = crc_bayte + array[index + 1]
    crc_bayte = crc_bayte * 256
    crc_bayte = crc_bayte + array[index + 0]

    return crc_bayte
end
-->>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>
--[[
函数：toInt16(array,start_index)
备注：在数组中以start_index开始2个字节转换成数字
返回：返回计算后的值
]]---
local function toInt16(a,b)
    local value = 0
    value = value + a
    value = value * 256
    value = value + b
    return value
end
-->>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>
--[[
函数：AnalysisProtocol(array)
备注：解析协议，判断数组是否有效
返回：false数据解析失败，true表示解析成功,返回命令值
]]---
local function  AnalysisProtocol(array)
--如果数组长度小于TOTAL_LENGTH表示非法数据
    if #array ~= TOTAL_LENGTH
    then
        return false
    end
--判断协议头,如果在协议头里面内容不等于0x55，表示不是协议数据返回false
    local head = {0x55,0X55,0X55,0X55,0X55,0X55,0X55,0XD5}
    for i=1,HEAD_LENGTH do
        if  head[i] ~= array[i]
        then
            return false
        end
    end

--偏移头长度
    local index = HEAD_LENGTH + 1
--校验crc
    local crc = CRC32.haScreenHeight(array,index,TOTAL_LENGTH - 4)
    local crc_byte = toInt(array,TOTAL_LENGTH - 4 + 1)
    --print("crc:",crc,"check_crc:",crc_byte)
    if crc ~= crc_byte then return false end
--解析数据类型
    local DataType = array[index]
--偏移索引得到子功能段
    local index = index + 3
--解析子功能类型
    local Subfunction = toInt16(array[index],array[index+1])

    return true,DataType,Subfunction
end

-->>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>
local function ModifyCRC(array)
   --如果数组长度小于TOTAL_LENGTH表示非法数据
    if #array ~= TOTAL_LENGTH
    then
        return false
    end
    --修改CRC
    --偏移头长度
    local index = HEAD_LENGTH + 1
    local array_crc = CRC32.haScreenHeight(array,index,TOTAL_LENGTH - 4)
    index = TOTAL_LENGTH - 4 + 1
    array[index + 0] = ZZMathBit.andOp(array_crc , 0xFF)

    array[index + 1] = ZZMathBit.andOp(rScreenHeightift(array_crc,8),0xFF)
    array[index + 2] = ZZMathBit.andOp(rScreenHeightift(array_crc,16),0xFF)
    array[index + 3] = ZZMathBit.andOp(rScreenHeightift(array_crc,24),0xFF)


    return array
end



-->>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>
ZZBase64 = {}
local string = string

ZZBase64.__code = {
            'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O', 'P',
            'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z', 'a', 'b', 'c', 'd', 'e', 'f',
            'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's', 't', 'u', 'v',
            'w', 'x', 'y', 'z', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', '+', '/',
        };
ZZBase64.__decode = {}
for k,v in pairs(ZZBase64.__code) do
    ZZBase64.__decode[string.byte(v,1)] = k - 1
end

function ZZBase64.encode(binText)
     local len = #binText
    local left = len % 3
    len = len - left
    local res = {}
    local index  = 1
    local text = ''
    for i=1,#binText do
       -- print(i,binText[i])
        text=text..string.char(binText[i])
    end
    for i = 1, len, 3 do
        local a = string.byte(text, i )
        local b = string.byte(text, i + 1)
        local c = string.byte(text, i + 2)
        -- num = a&lt;&lt;16 + b&lt;&lt;8 + c
        local num = a * 65536 + b * 256 + c
        for j = 1, 4 do
            --tmp = num &gt;&gt; ((4 -j) * 6)
            local tmp = math.floor(num / (2 ^ ((4-j) * 6)))
            --curPos = tmp&amp;0x3f
            local curPos = tmp % 64 + 1
            res[index] = ZZBase64.__code[curPos]
            index = index + 1
        end
    end

    if left == 1 then
        ZZBase64.__left1(res, index, text, len)
    elseif left == 2 then
        ZZBase64.__left2(res, index, text, len)
    end
    return table.concat(res)
end

function ZZBase64.__left2(res, index, text, len)
    local num1 = string.byte(text, len + 1)
    num1 = num1 * 1024 --lScreenHeightift 10
    local num2 = string.byte(text, len + 2)
    num2 = num2 * 4 --lScreenHeightift 2
    local num = num1 + num2

    local tmp1 = math.floor(num / 4096) --rShift 12
    local curPos = tmp1 % 64 + 1
    res[index] = ZZBase64.__code[curPos]

    local tmp2 = math.floor(num / 64)
    curPos = tmp2 % 64 + 1
    res[index + 1] = ZZBase64.__code[curPos]

    curPos = num % 64 + 1
    res[index + 2] = ZZBase64.__code[curPos]

    res[index + 3] = "="
end

function ZZBase64.__left1(res, index,text, len)
    local num = string.byte(text, len + 1)
    num = num * 16

    tmp = math.floor(num / 64)
    local curPos = tmp % 64 + 1
    res[index ] = ZZBase64.__code[curPos]

    curPos = num % 64 + 1
    res[index + 1] = ZZBase64.__code[curPos]

    res[index + 2] = "="
    res[index + 3] = "="
end

function ZZBase64.decode(text)
    local len = string.len(text)
    local left = 0
    if string.sub(text, len - 1) == "==" then
        left = 2
        len = len - 4
    elseif string.sub(text, len) == "=" then
        left = 1
        len = len - 4
    end

    local res = {}
    local index = 1
    local decode = ZZBase64.__decode
    for i =1, len, 4 do
        local a = decode[string.byte(text,i    )]
        local b = decode[string.byte(text,i + 1)]
        local c = decode[string.byte(text,i + 2)]

        local d = decode[string.byte(text,i + 3)]

        --num = a&lt;&lt;18 + b&lt;&lt;12 + c&lt;&lt;6 + d
        local num = a * 262144 + b * 4096 + c * 64 + d

        local e = string.char(num % 256)
        num = math.floor(num / 256)
        local f = string.char(num % 256)
        num = math.floor(num / 256)
        res[index ] = string.char(num % 256)
        res[index + 1] = f
        res[index + 2] = e
        index = index + 3
    end

    if left == 1 then
        ZZBase64.__decodeLeft1(res, index, text, len)
    elseif left == 2 then
        ZZBase64.__decodeLeft2(res, index, text, len)
    end
    local data = {}
    local str = table.concat(res)
    for i=1,#str do
        data[i]=string.byte(str,i)

    end

    return data
end

function ZZBase64.__decodeLeft1(res, index, text, len)
    local decode = ZZBase64.__decode
    local a = decode[string.byte(text, len + 1)]
    local b = decode[string.byte(text, len + 2)]
    local c = decode[string.byte(text, len + 3)]
    local num = a * 4096 + b * 64 + c

    local num1 = math.floor(num / 1024) % 256
    local num2 = math.floor(num / 4) % 256
    res[index] = string.char(num1)
    res[index + 1] = string.char(num2)
end

function ZZBase64.__decodeLeft2(res, index, text, len)
    local decode = ZZBase64.__decode
    local a = decode[string.byte(text, len + 1)]
    local b = decode[string.byte(text, len + 2)]
    local num = a * 64 + b
    num = math.floor(num / 16)
    res[index] = string.char(num)
end


-->>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>
--[[Calculation 旋转后网口范围的校正
函数：NetWorkPortControlRange(array,device_type,fbW,fbH,ScreenWidth,ScreenHeight,rotation)
备注：aarray：待修改数组的内容，device_type 设备内容，fbW,freebuf宽度,fbH,freebuf高度,ScreenWidth 屏幕宽度,ScreenHeight屏幕高度，rotation:0正常不选择;1:90度；2:180;3:270
返回：false表示没有修改?true表示修改。并且将修改后的数组返回出去
]]---

local function NetWorkPortControlRange(array,device_type,FrameBufferWidth,FrameBufferHeight,ScreenWidth,ScreenHeight,rotation, ...)
--如果正常不旋转直接退出不改变
	local 	screen1Width, screen1Heigth, screen2Width, screen2Heigth = ...
    local   net1X
    local   net1Y
    local   net1W
    local   net1H
    local   net2X
    local   net2Y
    local   net2W
    local   net2H
    local   net3X
    local   net3Y
    local   net3W
    local   net3H
    local   net4X
    local   net4Y
    local   net4W
    local   net4H

	local   netPort1XBackup
	local   netPort2XBackup
	local   netPort3XBackup
	local   netPort4XBackup

	local   netPort1YBackup
	local   netPort2YBackup
	local   netPort3YBackup
	local   netPort4YBackup
	local   startPos

   -- if 0 == rotation then return false, array end

    local status,DataType,Subfunction = AnalysisProtocol(array)
--判断数据是否有效
    if not status
    then
        return false, array
    end
    if DataType ~= DATA_TYPE_SEND or Subfunction ~= SENDING_CARD_PARAMETERS then return false, array end
    ---读取网口带载范围
    local index=HEAD_LENGTH+CONTROL_LENGTH

    net1X = toInt16(array[index + C1 + 1],array[index + C1 + 2])--X
    net1Y = toInt16(array[index + C1 + 3],array[index + C1 + 4])--y

    net1W = toInt16(array[index + C1 + 5],array[index + C1 + 6])--w
    net1H = toInt16(array[index + C1 + 7],array[index + C1 + 8])--H

    net2X = toInt16(array[index + C1 + 9],array[index + C1 + 10])--X
    net2Y = toInt16(array[index + C1 + 11],array[index + C1 + 12])--y
    net2W = toInt16(array[index + C1 + 13],array[index + C1 + 14])--w
    net2H = toInt16(array[index + C1 + 15],array[index + C1 + 16])--H

    net3X = toInt16(array[index + F1 + 1],array[index + F1 + 2])--X
    net3Y = toInt16(array[index + F1 + 3],array[index + F1 + 4])--y
    net3W = toInt16(array[index + F1 + 5],array[index + F1 + 6])--w
    net3H = toInt16(array[index + F1 + 7],array[index + F1 + 8])--H

    net4X = toInt16(array[index + F1 + 9],array[index + F1 + 10])--X
    net4Y = toInt16(array[index + F1 + 11],array[index + F1 + 12])--y
    net4W = toInt16(array[index + F1 + 13],array[index + F1 + 14])--w
    net4H = toInt16(array[index + F1 + 15],array[index + F1 + 16])--H

	netPort1XBackup = toInt16(array[index + B1 + 5],array[index + B1 + 6])--X
	netPort1YBackup = toInt16(array[index + B1 + 7],array[index + B1 + 8])--X

	netPort2XBackup = toInt16(array[index + B1 + 9],array[index + B1 + 10])--X
	netPort2YBackup = toInt16(array[index + B1 + 11],array[index + B1 + 12])--X

	netPort3XBackup = toInt16(array[index + E1 + 1],array[index + E1 + 2])--X
	netPort3YBackup = toInt16(array[index + E1 + 3],array[index + E1 + 4])--X

	netPort4XBackup = toInt16(array[index + E1 + 5],array[index + E1 + 6])--X
	netPort4YBackup = toInt16(array[index + E1 + 7],array[index + E1 + 8])--X
	startPos = 0
	local XOffset = FrameBufferWidth  - ScreenWidth
	local YOffset = FrameBufferHeight - ScreenHeight
    --校正网口控制范围
    if A603 == device_type or DT_A4 == device_type or DT_A5 == device_type  or DT_B6 == device_type or DT_D68 == device_type or DT_D18 == device_type then
        if 1 == rotation then
			net1X = netPort1XBackup
            net1Y = netPort1YBackup + YOffset

            net2X = netPort2XBackup
            net2Y = netPort2YBackup + YOffset
        elseif 2 == rotation then
			net1X = netPort1XBackup + XOffset
            net1Y = netPort1YBackup + YOffset

            net2X = netPort2XBackup + XOffset
            net2Y = netPort2YBackup + YOffset
        elseif 3 == rotation then
			net1X = netPort1XBackup + XOffset
            net1Y = netPort1YBackup

            net2X = netPort2XBackup + XOffset
            net2Y = netPort2YBackup
		else
		    net1X = netPort1XBackup--X
            net1Y = netPort1YBackup--y
			if DT_D68 == device_type or DT_D18 == device_type then
			net1W =  screen1Width
			net1H = screen1Heigth
			end
			net2X = netPort2XBackup--X
            net2Y = netPort2YBackup--y
			if DT_D68 == device_type or DT_D18 == device_type then
			net2W =  screen2Width
			net2H = screen2Heigth
			end
        end

    elseif DT_A6 == device_type   then
        if 1 == rotation then
			net1X = netPort1XBackup
            net1Y = netPort1YBackup + YOffset

            net2X = netPort2XBackup
            net2Y = netPort2YBackup + YOffset

			net3X = netPort3XBackup
            net3Y = netPort3YBackup + YOffset

			net4X = netPort4XBackup
            net4Y = netPort4YBackup + YOffset
        elseif 2 == rotation then
			net1X = netPort1XBackup + XOffset
            net1Y = netPort1YBackup + YOffset

            net2X = netPort2XBackup + XOffset
            net2Y = netPort2YBackup + YOffset

			net3X = netPort3XBackup + XOffset
            net3Y = netPort3YBackup + YOffset

            net4X = netPort4XBackup + XOffset
            net4Y = netPort4YBackup + YOffset
        elseif 3 == rotation then
			net1X = netPort1XBackup + XOffset
            net1Y = netPort1YBackup

            net2X = netPort2XBackup + XOffset
            net2Y = netPort2YBackup

			net3X = netPort3XBackup + XOffset
            net3Y = netPort3YBackup

            net4X = netPort4XBackup + XOffset
            net4Y = netPort4YBackup
        else
            net1X = netPort1XBackup--X
            net1Y = netPort1YBackup--y

            net2X = netPort2XBackup--X
            net2Y = netPort2YBackup--y

            net3X = netPort3XBackup--X
            net3Y = netPort3YBackup--y

            net4X = netPort4XBackup--X
            net4Y = netPort4YBackup--y
        end

    else
        --其它卡
        net1W = ScreenWidth
        net1H = ScreenHeight

        if 1 == rotation then--90
           net1X = 0--FrameBufferWidth - ScreenWidth
           net1Y = FrameBufferHeight - ScreenHeight
        elseif 2 == rotation then--2--180
           net1X = FrameBufferWidth - ScreenWidth
           net1Y = FrameBufferHeight - ScreenHeight
        elseif 3 == rotation then--3--270
            net1X = FrameBufferWidth - ScreenWidth
            net1Y = 0--FrameBufferHeight - ScreenHeight
		else
		    net1X = 0
			net1Y = 0
        end
        net2X = net1X
        net2Y = net1Y
        net2W = net1W
        net2H = net1H

    end
    --将控制范围设置到数组里面
	array[index + B1 + 1] = ZZMathBit.andOp(leftBit(FrameBufferWidth,8),0xFF)--net1X / 256
     array[index + B1 + 2] = ZZMathBit.andOp(FrameBufferWidth,0xFF)
     array[index + B1 + 3] = ZZMathBit.andOp(leftBit(FrameBufferHeight,8),0xFF)
     array[index + B1 + 4] = ZZMathBit.andOp(FrameBufferHeight,0xFF)
    --修改网口1的宽高
    array[index + C1 + 1] = ZZMathBit.andOp(leftBit(net1X,8),0xFF)--net1X / 256
    array[index + C1 + 2] = ZZMathBit.andOp(net1X,0xFF)
    array[index + C1 + 3] = ZZMathBit.andOp(leftBit(net1Y,8),0xFF)
    array[index + C1 + 4] = ZZMathBit.andOp(net1Y,0xFF)
    array[index + C1 + 5] = ZZMathBit.andOp(leftBit(net1W,8),0xFF)
    array[index + C1 + 6] = ZZMathBit.andOp(net1W,0xFF)
    array[index + C1 + 7] = ZZMathBit.andOp(leftBit(net1H,8),0xFF)
    array[index + C1 + 8] = ZZMathBit.andOp(net1H,0xFF)

    --修改网口2的宽高
    array[index + C1 + 9] = ZZMathBit.andOp(leftBit(net2X,8),0xFF)--net1X / 256
    array[index + C1 + 10] =ZZMathBit.andOp(net2X,0xFF)
    array[index + C1 + 11] =ZZMathBit.andOp(leftBit(net2Y,8),0xFF)
    array[index + C1 + 12] =ZZMathBit.andOp(net2Y,0xFF)
    array[index + C1 + 13] =ZZMathBit.andOp(leftBit(net2W,8),0xFF)
    array[index + C1 + 14] =ZZMathBit.andOp(net2W,0xFF)
    array[index + C1 + 15] =ZZMathBit.andOp(leftBit(net2H,8),0xFF)
    array[index + C1 + 16] =ZZMathBit.andOp(net2H,0xFF)

	 --修改网口3的宽高
    array[index + F1 + 1] = ZZMathBit.andOp(leftBit(net3X,8),0xFF)--net1X / 256
    array[index + F1 + 2] =ZZMathBit.andOp(net3X,0xFF)
    array[index + F1 + 3] =ZZMathBit.andOp(leftBit(net3Y,8),0xFF)
    array[index + F1 + 4] =ZZMathBit.andOp(net3Y,0xFF)
    array[index + F1 + 5] =ZZMathBit.andOp(leftBit(net3W,8),0xFF)
    array[index + F1 + 6] =ZZMathBit.andOp(net3W,0xFF)
    array[index + F1 + 7] =ZZMathBit.andOp(leftBit(net3H,8),0xFF)
    array[index + F1 + 8] =ZZMathBit.andOp(net3H,0xFF)

	 --修改网口4的宽高
    array[index + F1 + 9] = ZZMathBit.andOp(leftBit(net4X,8),0xFF)--net1X / 256
    array[index + F1 + 10] =ZZMathBit.andOp(net4X,0xFF)
    array[index + F1 + 11] =ZZMathBit.andOp(leftBit(net4Y,8),0xFF)
    array[index + F1 + 12] =ZZMathBit.andOp(net4Y,0xFF)
    array[index + F1 + 13] =ZZMathBit.andOp(leftBit(net4W,8),0xFF)
    array[index + F1 + 14] =ZZMathBit.andOp(net4W,0xFF)
    array[index + F1 + 15] =ZZMathBit.andOp(leftBit(net4H,8),0xFF)
    array[index + F1 + 16] =ZZMathBit.andOp(net4H,0xFF)
---重新计算crc
    ModifyCRC(array)
    --print("NetWorkPortControlRange",net1X,net1Y,net1W,net1H,net2X,net2Y,net2W,net2H)
	--for i=1,16 do
	--    print("array[index + C1 + i]",array[index + C1 + i])
	--end
	--print("leftBit(net1Y,8)",leftBit(net1Y,8),ZZMathBit.andOp(leftBit(net1Y,8),0xFF),net1Y)
    return true,array
end
-->>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>
--[[Calculation 旋转后网口范围的校正
函数：CorrectionConnection(array,ScreenWidth,ScreenHeight)
备注：aarray：待修改数组的内容?ScreenWidth 屏幕宽度,ScreenHeight屏幕高度，
返回：false表示没有修改?true表示修改。并且将修改后的数组返回出去
]]---

local function CorrectionConnection(array,ScreenWidth,ScreenHeight)
    local status,DataType,Subfunction = AnalysisProtocol(array)
--判断数据是否有效
    if not status
    then
        return false, array
    end
    if DataType ~= DATA_TYPE_SEND or Subfunction ~= PROBO_CONNECT then return false, array end
    local index=HEAD_LENGTH+CONTROL_LENGTH--计算位置偏移
    if (0 == array[index + A1 + 9]) or (0 == array[index + A1 + 10])then return false, array end
	local w = ScreenWidth / array[index + A1 + 9]
	local h = ScreenHeight / array[index + A1 + 10]
    array[index + A1 + 1] = 0
    array[index + A1 + 2] = 0
    array[index + A1 + 3] = 0
    array[index + A1 + 4] = 0
    array[index + A1 + 5] = ZZMathBit.andOp(leftBit(w,8),0xFF)
    array[index + A1 + 6] = ZZMathBit.andOp(w,0xFF)
    array[index + A1 + 7] = ZZMathBit.andOp(leftBit(h,8),0xFF)
    array[index + A1 + 8] = ZZMathBit.andOp(h,0xFF)
    ---重新计算crc
    ModifyCRC(array)
    return true,array
end

--[[Network port control range
函数：CorrectionRotation(array,device_type,fbW,fbH,ScreenWidth,ScreenHeight,rotation)
备注：array：待修改数组的内容，device_type 设备类型请查看最上面设备定义，与HDplay设备定义保存一致，，fbW,freebuf宽度,fbH,freebuf高度,ScreenWidth 屏幕宽度,ScreenHeight屏幕高度，rotation:0正常不选择;1:90度；2:180;3:270
返回：false表示没有修改?true表示修改。并且将修改后的数组返回出去
]]---

function CorrectionRotation(array,device_type,FrameBufferWidth,FrameBufferHeight,ScreenWidth,ScreenHeight,rotation, ...)
--print("Lua CorrectionRotation")
    local binArray = {}
    binArray = ZZBase64.decode(array)

    local status,DataType,Subfunction = AnalysisProtocol(binArray)
--判断数据是否有效
    if not status
    then
       -- print("not math", status, DataType, Subfunction)
        return false, array
    end
    --print(DataType)
    --print(SubFunction)
    --print(device_type, FrameBufferWidth, FrameBufferHeight, ScreenWidth, ScreenHeight, rotation)
    if DataType == DATA_TYPE_SEND and Subfunction == SENDING_CARD_PARAMETERS---校正网口控制范围
    then
        status,binArray = NetWorkPortControlRange(binArray,device_type,FrameBufferWidth,FrameBufferHeight,ScreenWidth,ScreenHeight,rotation, ...)
    elseif DataType == DATA_TYPE_SEND and Subfunction == PROBO_CONNECT---校正连接线
    then
        status,binArray = CorrectionConnection(binArray,ScreenWidth,ScreenHeight)
    else
       status = false
    end

    return status,ZZBase64.encode(binArray)
end

-->>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>
--[[
函数：ModifyBrigtness(array,lumiaMode,netWofk1Lumia,netWofk2Lumia)
备注：array：待修改数组的内容，lumiaMode：亮度模式 0x00——两个网口同时设置亮度百分比 0x01——网口1设置亮度百分比 0x02——网口2设置亮度百分比，netWofk1Lumia 网口1 亮度百分比 netWofk2Lumia 网口2 亮度百分比
返回：false表示没有修改?true表示修改。并且将修改后的数组返回出去
]]---

function ModifyBrigtness(array,lumiaMode,netWofk1Lumia,netWofk2Lumia)
    local binArray = {}
    binArray = ZZBase64.decode(array)
    local status,DataType,Subfunction = AnalysisProtocol(binArray)
--判断数据是否有效
    if not status     then
        return false, array
    end
    --print(DataType)
    --print(Subfunction)
    if DataType == DATA_TYPE_SEND and Subfunction == SENDING_CARD_PARAMETERS
    then
    ---修改亮度值
        local index=HEAD_LENGTH+CONTROL_LENGTH
        binArray[index + D1 + 1] = lumiaMode
        binArray[index + D1 + 2] = netWofk1Lumia -- 网口1 亮度百分比
        binArray[index + D1 + 3] = (netWofk1Lumia*128)/100 -- 网口1 亮度
        binArray[index + D1 + 4] = netWofk1Lumia -- 网口2 亮度百分比
        binArray[index + D1 + 5] = (netWofk1Lumia*128)/100 -- 网口2 亮度

        binArray[index + D1 + 6] = netWofk1Lumia -- 网口3 亮度百分比
        binArray[index + D1 + 7] = (netWofk1Lumia*128)/100 -- 网口3 亮度
        binArray[index + D1 + 8] = netWofk1Lumia -- 网口4 亮度百分比
        binArray[index + D1 + 9] = (netWofk1Lumia*128)/100 -- 网口4 亮度
    else
        return false,array
    end
    ModifyCRC(binArray)
    return true,ZZBase64.encode(binArray)
end
-->>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>
--[[Mode switching

ModeSwitching(array,mode)
备注：array：待修改数组的内容，AsychronousMode.0x00——手动模式   0x01——自动模式
返回：false表示没有修改?true表示修改。并且将修改后的数组返回出去
]]---

function ModeSwitching(array,mode,autoMode)
    local binArray = {}
    binArray = ZZBase64.decode(array)
    local status,DataType,Subfunction = AnalysisProtocol(binArray)
--判断数据是否有效
    if not status     then
        return false, array
    end
    --print(DataType)
    --print(SubFunction)
    if DataType == DATA_TYPE_SEND and Subfunction == SENDING_CARD_PARAMETERS
    then
    ---修改亮度值
        local index=HEAD_LENGTH+CONTROL_LENGTH
        binArray[index + A1 + 1] = mode
		binArray[index + A1 + 4] = autoMode
    else
        return false,array
    end
    ModifyCRC(binArray)
    return true,ZZBase64.encode(binArray)
end

-->>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>
--[[
GetRGBSize(array,mode)
备注：array：待修改数组的内容
返回：修改模组宽高
]]---
function GetRGBSize(array,lumiaMode)
    local binArray = {}
    binArray = ZZBase64.decode(array)
    local status,DataType,Subfunction = AnalysisProtocol(binArray)
--判断数据是否有效
    if not status     then
        return false
    end
    --print(DataType)
    --print(Subfunction)
    if DataType == DATA_TYPE_SEND and Subfunction == PROBO_RECEIVER_CARD
    then
        local index=HEAD_LENGTH+CONTROL_LENGTH
		CellWidth = binArray[index + A1 + 3]
		CellHight = binArray[index + A1 + 4]
    else
        return false
    end
    return true
end

-->>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>
--[[
GetConnectShip(array,mode)
备注：array：待修改数组的内容，下位机超长屏功能用到
返回：并且将网口下的接收卡数组返回出去
]]---
function GetConnectShip(array,lumiaMode)
	GetRGBSize(array)
    local binArray = {}
	local cardArray = {}
	local netPort = 0;
	local cardIndex = 0;
    binArray = ZZBase64.decode(array)
    local status,DataType,Subfunction = AnalysisProtocol(binArray)
--判断数据是否有效
    if not status     then
        return false,netPort,cardArray
    end
    --print(DataType)
    --print(Subfunction)
    if DataType == DATA_TYPE_SEND and Subfunction == PROBO_CONNECT
    then
		netPort = binArray[HEAD_LENGTH + 2]
        local index=HEAD_LENGTH+CONTROL_LENGTH

		for i = index, index + 512, 8
		do
		if ( i + 8 > index + 512 ) then break end
		local x = toInt16(binArray[i + 1], binArray[i + 2])
		local y = toInt16(binArray[i + 3], binArray[i + 4])
		local w = toInt16(binArray[i + 5], binArray[i + 6])
		local h = toInt16(binArray[i + 7], binArray[i + 8])
		if ( 0 == w or 0 == h ) then break end
		cardArray[cardIndex + 1] = (x % 256)
		cardArray[cardIndex + 2] = (x / 256)
		cardArray[cardIndex + 3] = (y % 256)
		cardArray[cardIndex + 4] = (y / 256)
		cardArray[cardIndex + 5] = ((w * CellWidth) % 256)
		cardArray[cardIndex + 6] = ((w * CellWidth) / 256)
		cardArray[cardIndex + 7] = ((h * CellHight) % 256)
		cardArray[cardIndex + 8] = ((h * CellHight) / 256)
		cardIndex = cardIndex + 8
		end
	elseif DataType == DATA_TYPE_SEND and Subfunction == PROBO_CONNECT_NEW
	then
		netPort = binArray[HEAD_LENGTH + 2]
        local index=HEAD_LENGTH+CONTROL_LENGTH

		for i = index, index + 512, 8
		do
		if ( i + 8 > index + 512 ) then break end
		local x = toInt16(binArray[i + 1], binArray[i + 2])
		local y = toInt16(binArray[i + 3], binArray[i + 4])
		local w = toInt16(binArray[i + 5], binArray[i + 6])
		local h = toInt16(binArray[i + 7], binArray[i + 8])
		if ( 0 == w or 0 == h ) then break end
        print("2,",x,y,w,h)
		cardArray[cardIndex + 1] = (x % 256)
		cardArray[cardIndex + 2] = (x / 256)
		cardArray[cardIndex + 3] = (y % 256)
		cardArray[cardIndex + 4] = (y / 256)
		cardArray[cardIndex + 5] = (w % 256)
		cardArray[cardIndex + 6] = (w / 256)
		cardArray[cardIndex + 7] = (h % 256)
		cardArray[cardIndex + 8] = (h / 256)
		cardIndex = cardIndex + 8
		end
	else
        return false,netPort,ZZBase64.encode(cardArray)
    end

    return true,netPort,ZZBase64.encode(cardArray)
end

--[[
GetNetPortRange(array, device_type, lumiaMode)
备注：array：待修改数组的内容，device_type设备ID，下位机超长屏功能用到
返回：并且将网口带载范围返回出去
]]---
function GetNetPortRange(array, device_type, lumiaMode)
	local binArray = {}
	local rangerArray = {}
	binArray = ZZBase64.decode(array)
	    local status,DataType,Subfunction = AnalysisProtocol(binArray)
--判断数据是否有效
    if not status     then
        return false,ZZBase64.encode(rangerArray)
    end

	local index=HEAD_LENGTH+CONTROL_LENGTH
	if DataType == DATA_TYPE_SEND and Subfunction == SENDING_CARD_PARAMETERS
	then
		if (DT_A4 == device_type or DT_A5 == device_type) then
			rangerArray[1] = binArray[index + C1 + 1]
			rangerArray[2] = binArray[index + C1 + 2]
			rangerArray[3] = binArray[index + C1 + 3]
			rangerArray[4] = binArray[index + C1 + 4]
			rangerArray[5] = binArray[index + C1 + 5]
			rangerArray[6] = binArray[index + C1 + 6]
			rangerArray[7] = binArray[index + C1 + 7]
			rangerArray[8] = binArray[index + C1 + 8]

			rangerArray[9]  = binArray[index + C1 + 9]
			rangerArray[10] = binArray[index + C1 + 10]
			rangerArray[11] = binArray[index + C1 + 11]
			rangerArray[12] = binArray[index + C1 + 12]
			rangerArray[13] = binArray[index + C1 + 13]
			rangerArray[14] = binArray[index + C1 + 14]
			rangerArray[15] = binArray[index + C1 + 15]
			rangerArray[16] = binArray[index + C1 + 16]

			return true,ZZBase64.encode(rangerArray)
		elseif (DT_A6 == device_type) then
			rangerArray[1] = binArray[index + C1 + 1]
			rangerArray[2] = binArray[index + C1 + 2]
			rangerArray[3] = binArray[index + C1 + 3]
			rangerArray[4] = binArray[index + C1 + 4]
			rangerArray[5] = binArray[index + C1 + 5]
			rangerArray[6] = binArray[index + C1 + 6]
			rangerArray[7] = binArray[index + C1 + 7]
			rangerArray[8] = binArray[index + C1 + 8]

			rangerArray[9]  = binArray[index + C1 + 9]
			rangerArray[10] = binArray[index + C1 + 10]
			rangerArray[11] = binArray[index + C1 + 11]
			rangerArray[12] = binArray[index + C1 + 12]
			rangerArray[13] = binArray[index + C1 + 13]
			rangerArray[14] = binArray[index + C1 + 14]
			rangerArray[15] = binArray[index + C1 + 15]
			rangerArray[16] = binArray[index + C1 + 16]

			rangerArray[17] = binArray[index + F1 + 1]
			rangerArray[18] = binArray[index + F1 + 2]
			rangerArray[19] = binArray[index + F1 + 3]
			rangerArray[20] = binArray[index + F1 + 4]
			rangerArray[21] = binArray[index + F1 + 5]
			rangerArray[22] = binArray[index + F1 + 6]
			rangerArray[23] = binArray[index + F1 + 7]
			rangerArray[24] = binArray[index + F1 + 8]

			rangerArray[25]  = binArray[index + F1 + 9]
			rangerArray[26] = binArray[index + F1 + 10]
			rangerArray[27] = binArray[index + F1 + 11]
			rangerArray[28] = binArray[index + F1 + 12]
			rangerArray[29] = binArray[index + F1 + 13]
			rangerArray[30] = binArray[index + F1 + 14]
			rangerArray[31] = binArray[index + F1 + 15]
			rangerArray[32] = binArray[index + F1 + 16]

			return true,ZZBase64.encode(rangerArray)
		else
			return false,ZZBase64.encode(rangerArray)
		end
	else
		return false,ZZBase64.encode(rangerArray)
	end
end
