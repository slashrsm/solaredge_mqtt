#!/bin/sh
if [ "$DEBUG" = "1" ]; then
    exec ./solaredge_mqtt -v
else
    exec ./solaredge_mqtt
fi
