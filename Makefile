# Compiler Settings
CC=cc
CFLAGS=-Wall -Wextra -Werror -Wcomment -Wformat -Wformat-security -Wmain -Wnonnull -Wunused -std=c11

# Source Code
SRC_CORE=./src/core/lib/*.c
SRC_CORE_APP=./src/core/app/*.c
SRC_LOGGER=./src/logger/lib/*.c

INCLUDE_DIR=./include
INCLUDE_CORE=$(INCLUDE_DIR)/*.h
INCLUDE_LOGGER=$(INCLUDE_DIR)/logger/*.h

TEST_CORE=./src/core/lib/test/main.c
LIBS_CORE=-ljson-c -lcurl -lgit2

TEST_LOGGER=./src/logger/lib/test/main.c
LIBS_LOGGER=-lwhy2

# Output Files
PROJECT_NAME=why2
OUTPUT=out

OUTPUT_TEST_CORE=$(OUTPUT)/$(PROJECT_NAME)-core-test
OUTPUT_APP=$(OUTPUT)/$(PROJECT_NAME)-app

OUTPUT_TEST_LOGGER=$(OUTPUT)/$(PROJECT_NAME)-logger-test

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
	$(MAKE) clean
	$(CC) $(CFLAGS) -fPIC -c $(SRC_CORE)
	$(CC) $(CFLAGS) -shared -o lib$(PROJECT_NAME).so *.o $(LIBS_CORE)

buildLibLogger:
	$(MAKE) clean
	$(CC) $(CFLAGS) -fPIC -c $(SRC_LOGGER)
	$(CC) $(CFLAGS) -shared -o lib$(PROJECT_NAME)-logger.so *.o $(LIBS_LOGGER)

installLibCore: buildLibCore
	install -m 755 ./lib$(PROJECT_NAME).so $(INSTALL_LIBRARY)/lib$(PROJECT_NAME).so

installAppCore: app
	install -m 755 $(OUTPUT_APP) $(INSTALL_BIN)/$(PROJECT_NAME)

installLibLogger: buildLibLogger
	install -m 755 ./lib$(PROJECT_NAME)-logger.so $(INSTALL_LIBRARY)/lib$(PROJECT_NAME)-logger.so

testCore:
	$(CC) $(CFLAGS) -g $(TEST_CORE) -o $(OUTPUT_TEST_CORE) -l$(PROJECT_NAME)

testLogger:
	$(CC) $(CFLAGS) -g $(TEST_LOGGER) -o $(OUTPUT_TEST_LOGGER) -l$(PROJECT_NAME)

app:
	$(CC) $(CFLAGS) $(SRC_CORE_APP) -o $(OUTPUT_APP) -l$(PROJECT_NAME)

clean:
	rm -rf $(OUTPUT)/* *.o *.so

installHeader: installHeaderCore installHeaderLogger
install: installHeader installLibCore installLibLogger installAppCore clean
installTest: install testCore testLogger
all: install