# Spike K5 guest: a screen that changes all the time. Build out of tree:
#   make ROBCO_CORE=<RobCo-Core checkout> BUILD=<dir> K5_W=640 K5_H=480

ifndef ROBCO_CORE
$(error set ROBCO_CORE to a RobCo-Core checkout)
endif
IMAGE      := k5
LINK_ADDR  := 0x8000
SRCS       := main.c
CONFIG_H   := robco_config.h
K5_W       ?= 640
K5_H       ?= 480
EXTRA_CFLAGS += -DK5_W=$(K5_W) -DK5_H=$(K5_H)

include $(ROBCO_CORE)/mk/robco.mk
