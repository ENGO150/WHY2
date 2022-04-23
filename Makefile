all: main

# Main file
files = src/test/main.c

# Source files
files += src/*.c

# Header files
files += include/*.h

main:
	@echo Compiling...
	cc $(files) -ljson-c -lcurl -o out/why2
