# -*- coding: utf-8 -*-
# preScript: метит точку входа плоского образа как функцию до автоанализа.
# @category MAX.RE

# Реальный статический вход игры: obj0.base(0x10000) + LE.eip(0xFC704);
# объекты LE 1-базовые. В рантайме загрузчик релоцирует объект (база XMS-блока),
# но оффсет внутри объекта тот же.
entry = 0x10C704
program = getCurrentProgram()
addr = toAddr(entry)
addEntryPoint(addr)
createFunction(addr, "entry")
print("entry marked at %s" % addr)
