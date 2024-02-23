# This is part of WHY2
# Copyright (C) 2022 Václav Šmejkal

# This program is free software: you can redistribute it and/or modify
# it under the terms of the GNU General Public License as published by
# the Free Software Foundation, either version 3 of the License, or
# (at your option) any later version.

# This program is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
# GNU General Public License for more details.

# You should have received a copy of the GNU General Public License
# along with this program.  If not, see <https://www.gnu.org/licenses/>.

# Compiler Settings
CC=cc
RC=cargo
CP=sudo install -m 755 # Not so compiler related but idk where to put it
RFLAGS=--manifest-path src/chat/config/Cargo.toml
CFLAGS=-Wall -Wextra -Werror -Wcomment -Wformat -Wformat-security -Wmain -Wnonnull -Wunused -std=gnu11 -O2 -g # Remove the '-g' flag if you want the smallest possible lib size

# Output Files
PROJECT_NAME=why2
OUTPUT=out
LOGS=logs

OUTPUT_TEST_CORE=$(OUTPUT)/$(PROJECT_NAME)-core-test
OUTPUT_APP_CORE=$(OUTPUT)/$(PROJECT_NAME)-core-app

OUTPUT_TEST_LOGGER=$(OUTPUT)/$(PROJECT_NAME)-logger-test
OUTPUT_APP_LOGGER=$(OUTPUT)/$(PROJECT_NAME)-logger-app

OUTPUT_CHAT_CLIENT=$(OUTPUT)/$(PROJECT_NAME)-chat-client
OUTPUT_CHAT_SERVER=$(OUTPUT)/$(PROJECT_NAME)-chat-server

LIB_CHAT_CONFIG_OUT=./src/chat/config/target/release

# Source Code
SRC_CORE=./src/core/lib/*.c ./src/core/lib/utils/*.c
SRC_CORE_APP=./src/core/app/*.c
SRC_LOGGER=./src/logger/lib/*.c
SRC_LOGGER_APP=./src/logger/app/*.c
SRC_CHAT_CLIENT=./src/chat/main/client.c
SRC_CHAT_SERVER=./src/chat/main/server.c
SRC_CHAT_MISC=./src/chat/*.c

INCLUDE_DIR=./include
INCLUDE_CORE=$(INCLUDE_DIR)/*.h
INCLUDE_LOGGER=$(INCLUDE_DIR)/logger/*.h
INCLUDE_CHAT=$(INCLUDE_DIR)/chat/*.h

TEST_CORE=./src/core/lib/test/main.c
LIBS_CORE=-ljson-c -lcurl -lgit2
LIB_CORE=-l$(PROJECT_NAME)

TEST_LOGGER=./src/logger/lib/test/main.c
LIBS_LOGGER=$(LIB_CORE)
LIB_LOGGER=-l$(PROJECT_NAME)-logger

LIBS_LIB_CHAT=$(LIB_CORE) -lpthread -lgmp
LIB_CHAT=-l$(PROJECT_NAME)-chat
LIB_CHAT_CONFIG=$(LIB_CHAT)-config
LIBS_CHAT=$(LIB_CHAT) $(LIBS_LIB_CHAT) $(LIB_CHAT_CONFIG)

# Install Files
INSTALL_INCLUDE=/usr/include
INSTALL_LIBRARY=/usr/lib
INSTALL_BIN=/usr/bin

# Misc
DOLLAR=$

##########

no_target: # Do not use this, please <3
	@echo Hey you... You have to enter your target, too. Use \'install\' target for installing $(PROJECT_NAME)-core.

check_root:
ifneq ($(BYPASS_CHECK),true)
	@if [ "$(shell id -u)" = 0 ]; then\
		echo "Do not run install rules as root.";\
		exit 1;\
	fi
endif

install_header_core:
	for file in $(INCLUDE_CORE); do $(CP) -D $(DOLLAR)file -t $(INSTALL_INCLUDE)/$(PROJECT_NAME); done
	sudo ln -sf $(INSTALL_INCLUDE)/$(PROJECT_NAME)/$(PROJECT_NAME).h $(INSTALL_INCLUDE)/$(PROJECT_NAME).h

install_header_logger:
	for file in $(INCLUDE_LOGGER); do $(CP) -D $(DOLLAR)file -t $(INSTALL_INCLUDE)/$(PROJECT_NAME)/logger; done

install_header_chat:
	for file in $(INCLUDE_CHAT); do $(CP) -D $(DOLLAR)file -t $(INSTALL_INCLUDE)/$(PROJECT_NAME)/chat; done

build_lib_core:
	$(MAKE) clean
	$(CC) $(CFLAGS) -fPIC -c $(SRC_CORE)
	$(CC) $(CFLAGS) -shared -o lib$(PROJECT_NAME).so *.o $(LIBS_CORE)

build_lib_logger:
	$(MAKE) clean
	$(CC) $(CFLAGS) -fPIC -c $(SRC_LOGGER)
	$(CC) $(CFLAGS) -shared -o lib$(PROJECT_NAME)-logger.so *.o $(LIBS_LOGGER)

build_chat_client:
	$(CC) $(CFLAGS) $(SRC_CHAT_CLIENT) -o $(OUTPUT_CHAT_CLIENT) $(LIBS_CHAT)

build_chat_server:
	$(CC) $(CFLAGS) $(SRC_CHAT_SERVER) -o $(OUTPUT_CHAT_SERVER) $(LIBS_CHAT)

build_lib_chat:
	$(MAKE) clean
	$(CC) $(CFLAGS) -fPIC -c $(SRC_CHAT_MISC)
	$(RC) build $(RFLAGS) --release
	$(CP) $(LIB_CHAT_CONFIG_OUT)/lib$(PROJECT_NAME)_chat_config.so $(INSTALL_LIBRARY)/lib$(PROJECT_NAME)-chat-config.so
	$(CC) $(CFLAGS) -shared -o lib$(PROJECT_NAME)-chat.so *.o $(LIBS_LIB_CHAT)

install_lib_core: build_lib_core
	$(CP) ./lib$(PROJECT_NAME).so $(INSTALL_LIBRARY)/lib$(PROJECT_NAME).so

install_app_core: app_core
	$(CP) $(OUTPUT_APP_CORE) $(INSTALL_BIN)/$(PROJECT_NAME)

install_lib_logger: build_lib_logger
	$(CP) ./lib$(PROJECT_NAME)-logger.so $(INSTALL_LIBRARY)/lib$(PROJECT_NAME)-logger.so

install_app_logger: app_logger
	$(CP) $(OUTPUT_APP_LOGGER) $(INSTALL_BIN)/$(PROJECT_NAME)-logger

install_lib_chat: build_lib_chat
	$(CP) ./lib$(PROJECT_NAME)-chat.so $(INSTALL_LIBRARY)/lib$(PROJECT_NAME)-chat.so

test_core:
	$(CC) $(CFLAGS) $(TEST_CORE) -o $(OUTPUT_TEST_CORE) $(LIB_CORE)

test_logger:
	$(CC) $(CFLAGS) $(TEST_LOGGER) -o $(OUTPUT_TEST_LOGGER) $(LIBS_LOGGER) $(LIB_LOGGER)

app_core:
	$(CC) $(CFLAGS) $(SRC_CORE_APP) -o $(OUTPUT_APP_CORE) $(LIB_CORE)

app_logger:
	$(CC) $(CFLAGS) $(SRC_LOGGER_APP) -o $(OUTPUT_APP_LOGGER) $(LIBS_LOGGER) $(LIB_LOGGER)

clean:
	$(RC) clean $(RFLAGS)
	rm -rf $(OUTPUT)/* $(LOGS)/* *.o *.so vgcore.*

build_chat: build_chat_server build_chat_client

install_header: install_header_core install_header_logger install_header_chat
install_libs: install_lib_core install_lib_logger install_lib_chat
install_apps: install_app_core install_app_logger
install: check_root install_header install_libs install_apps clean
install_test: install test_core test_logger
all: install