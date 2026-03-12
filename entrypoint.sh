#!/bin/sh
if [ "$DEBUG" = "1" ]; then
    exec python main.py -v
else
    exec python main.py
fi
