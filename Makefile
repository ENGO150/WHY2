all: main

files = src/main.c

main:
	cc $(files) -o out/why2