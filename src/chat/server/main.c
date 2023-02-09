#include <stdio.h>
#include <stdlib.h>
#include <sys/socket.h>
#include <sys/types.h>

#include <netinet/in.h>

int main(void)
{
    int servSockD = socket(AF_INET, SOCK_STREAM, 0); //CREATE SERVER SOCKET

    //TODO: REMOVE
    char serMsg[255] = "Message from the server to the "
                       "client \'Hello Client\' ";

    //DEFINE SERVER ADDRESS
    struct sockaddr_in servAddr;

    servAddr.sin_family = AF_INET;
    servAddr.sin_port = htons(9001);
    servAddr.sin_addr.s_addr = INADDR_ANY;

    //BIND SOCKET
    bind(servSockD, (struct sockaddr*) &servAddr, sizeof(servAddr));

    //LISTEN
    listen(servSockD, 1);

    //CLIENT SOCKET
    int clientSocket = accept(servSockD, NULL, NULL);

    char clientMsg[255];

    //SEND
    send(clientSocket, serMsg, sizeof(serMsg), 0);
    recv(clientSocket, clientMsg, sizeof(clientMsg), 0);

    printf("%s\n", clientMsg);

    return 0;
}