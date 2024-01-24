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
CP=sudo install -m 755
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

LIBS_LIB_CHAT=$(LIB_CORE) -lpthread
LIB_CHAT=-l$(PROJECT_NAME)-chat
LIB_CHAT_CONFIG=$(LIB_CHAT)-config
LIBS_CHAT=$(LIB_CHAT) $(LIBS_LIB_CHAT) -lyaml $(LIB_CHAT_CONFIG)

# Install Files
INSTALL_INCLUDE=/usr/include
INSTALL_LIBRARY=/usr/lib
INSTALL_BIN=/usr/bin

# Misc
DOLLAR=$

##########

noTarget: # Do not use this, please <3
	@echo Hey you... You have to enter your target, too. Use \'install\' target for installing $(PROJECT_NAME)-core.

installHeaderCore:
	for file in $(INCLUDE_CORE); do $(CP) -D $(DOLLAR)file -t $(INSTALL_INCLUDE)/$(PROJECT_NAME); done
	sudo ln -sf $(INSTALL_INCLUDE)/$(PROJECT_NAME)/$(PROJECT_NAME).h $(INSTALL_INCLUDE)/$(PROJECT_NAME).h

installHeaderLogger:
	for file in $(INCLUDE_LOGGER); do $(CP) -D $(DOLLAR)file -t $(INSTALL_INCLUDE)/$(PROJECT_NAME)/logger; done

installHeaderChat:
	for file in $(INCLUDE_CHAT); do $(CP) -D $(DOLLAR)file -t $(INSTALL_INCLUDE)/$(PROJECT_NAME)/chat; done

buildLibCore:
	$(MAKE) clean
	$(CC) $(CFLAGS) -fPIC -c $(SRC_CORE)
	$(CC) $(CFLAGS) -shared -o lib$(PROJECT_NAME).so *.o $(LIBS_CORE)

buildLibLogger:
	$(MAKE) clean
	$(CC) $(CFLAGS) -fPIC -c $(SRC_LOGGER)
	$(CC) $(CFLAGS) -shared -o lib$(PROJECT_NAME)-logger.so *.o $(LIBS_LOGGER)

buildChatClient:
	$(CC) $(CFLAGS) $(SRC_CHAT_CLIENT) -o $(OUTPUT_CHAT_CLIENT) $(LIBS_CHAT)

buildChatServer:
	$(CC) $(CFLAGS) $(SRC_CHAT_SERVER) -o $(OUTPUT_CHAT_SERVER) $(LIBS_CHAT)

buildLibChat:
	$(MAKE) clean
	$(CC) $(CFLAGS) -fPIC -c $(SRC_CHAT_MISC)
	$(RC) build $(RFLAGS) --release
	$(CP) $(LIB_CHAT_CONFIG_OUT)/lib$(PROJECT_NAME)_chat_config.so $(INSTALL_LIBRARY)/lib$(PROJECT_NAME)-chat-config.so
	$(CC) $(CFLAGS) -shared -o lib$(PROJECT_NAME)-chat.so *.o $(LIBS_LIB_CHAT)

installLibCore: buildLibCore
	$(CP) ./lib$(PROJECT_NAME).so $(INSTALL_LIBRARY)/lib$(PROJECT_NAME).so

installAppCore: appCore
	$(CP) $(OUTPUT_APP_CORE) $(INSTALL_BIN)/$(PROJECT_NAME)

installLibLogger: buildLibLogger
	$(CP) ./lib$(PROJECT_NAME)-logger.so $(INSTALL_LIBRARY)/lib$(PROJECT_NAME)-logger.so

installAppLogger: appLogger
	$(CP) $(OUTPUT_APP_LOGGER) $(INSTALL_BIN)/$(PROJECT_NAME)-logger

installLibChat: buildLibChat
	$(CP) ./lib$(PROJECT_NAME)-chat.so $(INSTALL_LIBRARY)/lib$(PROJECT_NAME)-chat.so

testCore:
	$(CC) $(CFLAGS) $(TEST_CORE) -o $(OUTPUT_TEST_CORE) $(LIB_CORE)

testLogger:
	$(CC) $(CFLAGS) $(TEST_LOGGER) -o $(OUTPUT_TEST_LOGGER) $(LIBS_LOGGER) $(LIB_LOGGER)

appCore:
	$(CC) $(CFLAGS) $(SRC_CORE_APP) -o $(OUTPUT_APP_CORE) $(LIB_CORE)

appLogger:
	$(CC) $(CFLAGS) $(SRC_LOGGER_APP) -o $(OUTPUT_APP_LOGGER) $(LIBS_LOGGER) $(LIB_LOGGER)

clean:
	$(RC) clean $(RFLAGS)
	rm -rf $(OUTPUT)/* $(LOGS)/* *.o *.so vgcore.*

buildChat: buildChatServer buildChatClient

installHeader: installHeaderCore installHeaderLogger installHeaderChat
installLibs: installLibCore installLibLogger installLibChat
installApps: installAppCore installAppLogger
install: installHeader installLibs installApps clean
installTest: install testCore testLogger
all: install