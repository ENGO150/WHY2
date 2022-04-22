all: main

# Main file
files = src/test/main.c

# Source files
files += src/*.c

# Header files
files += include/*.h

main:
	@echo Compiling...
	cc $(files) -ljson-c -lcurl -lm -o out/why2
