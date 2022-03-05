all: main

# Main file
files = src/test/main.c

# Source files
files += src/encrypter.c

# Header files
files += include/encrypter.h

main:
	cc $(files) -o out/why2