# WHY2 Encryption System

*Yeah.*

This project is made 'like a library', so compiling is useless... :)

### Table of contents

  - [Using in Your projects](#using-in-your-projects)
  - [Example of code](#example-of-code)
  - [Example programs](#example-programs)
  - WHY2 in another languages
    - [JavaScript](#javascript)

## Using in Your projects 

To **encrypt** text, use function `encryptText()` from file `include/encrypter.h`.

To **decrypt** text, use function `decryptText()` from file `include/decrypter.h`.

Jump to [examples](#examples) if you're not sure, how to use.

## Example of code

- Encryption:
    ```c
    //FIRST VARIANT
    char *yourText = encryptText("Put here text, you want encrypt...", "tzXlZGxkhfYOvRthqokDrmGFyDMylgmeIlrJTpVAwuqrLjABXM"); //The second thing is Your **key**. (The key must be 50 characters long!)

    //SECOND VARIANT
    char *yourText = encryptText("Put here text, you want encrypt...", NULL); //See? You don't have to use Your key. Program will automatically generate one for you. It will be printed out, so save it somewhere.
    ```

- Decryption:
    ```c
    char *yourText = decryptText("158.-83.9388.-14.57.8419.113.-98.10576", "tzXlZGxkhfYOvRthqokDrmGFyDMylgmeIlrJTpVAwuqrLjABXM"); //First parameter is Your encrypted text, the second is key you want to use for decryption it.
    ```

## Example programs

Uhm... There aren't any examples (for now)... I will maybe create some... Later...

## JavaScript

Yes, yes, yes... One awesome [guy](https://github.com/SebestikCZ) guy created this. You can found the JS remake [>> HERE <<](https://github.com/ENGO150/WHY2-Encryption-System/tree/js). Thanks, Sebestik <3