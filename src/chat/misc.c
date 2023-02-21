#include <why2/chat/misc.h>

#include <string.h>
#include <sys/socket.h>

#include <why2/memory.h>

void send_socket(char *text, int socket)
{
    unsigned short text_length = (unsigned short) strlen(text);
    char *final = why2_calloc(strlen(text) + 2, sizeof(char));

    //SPLIT LENGTH INTO TWO CHARS
    final[0] = (unsigned) text_length & 0xff;
    final[1] = (unsigned) text_length >> 8;

    for (int i = 2; i < text_length + 2; i++) //APPEND
    {
        final[i] = text[i - 2];
    }

    //SEND
    send(socket, final, text_length + 2, 0);

    why2_deallocate(final);
}