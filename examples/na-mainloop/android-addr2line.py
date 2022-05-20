#!/usr/bin/python3
import sys
import re

for line in sys.stdin:
    search = re.search('#[0-9]+ +pc +([0-9A-Fa-f]+) +', line)
    if search != None:
        print(search.group(1))
