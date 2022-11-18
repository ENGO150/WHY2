# Compiler Settings
CC=cc
CFLAGS=-Wall -Wextra -Werror -Wcomment -Wformat -Wformat-security -Wmain -Wnonnull -Wunused -std=c11

# Source Code
SRC_CORE=./src/core/lib/*.c
SRC_CORE_APP=./src/core/app/*.c
SRC_LOGGER=./src/logger/*.c

INCLUDE_DIR=./include
INCLUDE_CORE=$(INCLUDE_DIR)/*.h
INCLUDE_LOGGER=$(INCLUDE_DIR)/logger/*.h

TEST=./src/lib/test/main.c
LIBS=-ljson-c -lcurl -lgit2

# Output Files
PROJECT_NAME=why2
OUTPUT=out
OUTPUT_TEST=$(OUTPUT)/$(PROJECT_NAME)-test
OUTPUT_APP=$(OUTPUT)/$(PROJECT_NAME)-app

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
	for file in $(INCLUDE_CORE); do install -m 755 -D $(DOLLAR)file -t $(INSTALL_INCLUDE)/$(PROJECT_NAME); done
	ln -sf $(INSTALL_INCLUDE)/$(PROJECT_NAME)/$(PROJECT_NAME).h $(INSTALL_INCLUDE)/$(PROJECT_NAME).h

installHeaderLogger:
	for file in $(INCLUDE_LOGGER); do install -m 755 -D $(DOLLAR)file -t $(INSTALL_INCLUDE)/$(PROJECT_NAME)/logger; done

buildLibCore:
	$(CC) $(CFLAGS) -fPIC -c $(SRC_CORE)
	$(CC) $(CFLAGS) -shared -o lib$(PROJECT_NAME).so *.o $(LIBS)

installLibCore: buildLibCore
	install -m 755 ./lib$(PROJECT_NAME).so $(INSTALL_LIBRARY)/lib$(PROJECT_NAME).so

installApp: app
	install -m 755 $(OUTPUT_APP) $(INSTALL_BIN)/$(PROJECT_NAME)

test:
	$(CC) $(CFLAGS) -g $(TEST) -o $(OUTPUT_TEST) -l$(PROJECT_NAME)

app:
	$(CC) $(CFLAGS) $(SRC_CORE_APP) -o $(OUTPUT_APP) -l$(PROJECT_NAME)

clean:
	rm -rf $(OUTPUT)/* *.o *.so

installHeader: installHeaderCore installHeaderLogger
install: installHeader installLibCore installApp clean
installTest: install test
all: install