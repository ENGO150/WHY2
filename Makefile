# Compiler Settings
CC=cc
CFLAGS=-Wall -Wextra -Werror -Wcomment -Wformat -Wformat-security -Wmain -Wnonnull -Wunused -std=c11

# Source Code
SRC=src/lib/*.c
SRC_APP=src/app/*.c
INCLUDE_DIR=include
INCLUDE=$(INCLUDE_DIR)/*.h
TEST=src/lib/test/main.c
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

installHeader:
	for file in $(INCLUDE); do install -m 755 -D $(DOLLAR)file -t $(INSTALL_INCLUDE)/$(PROJECT_NAME); done
	install -m 755 $(INCLUDE_DIR)/$(PROJECT_NAME).h $(INSTALL_INCLUDE)/$(PROJECT_NAME).h

installLib:
	$(CC) $(CFLAGS) -fPIC -c $(SRC)
	$(CC) $(CFLAGS) -shared -o lib$(PROJECT_NAME).so *.o $(LIBS)
	install -m 755 lib$(PROJECT_NAME).so $(INSTALL_LIBRARY)/lib$(PROJECT_NAME).so

installApp: app
	install -m 755 $(OUTPUT_APP) $(INSTALL_BIN)/$(PROJECT_NAME)

test:
	$(CC) $(CFLAGS) -g $(TEST) -o $(OUTPUT_TEST) -l$(PROJECT_NAME)

app:
	$(CC) $(CFLAGS) $(SRC_APP) -o $(OUTPUT_APP) -l$(PROJECT_NAME)

clean:
	rm -rf $(OUTPUT)/* *.o *.so

all: install
installTest: install test
install: installHeader installLib installApp clean