#include <stdio.h>
#include <string.h>

#include <why2.h>

int main(void)
{
    why2_log_file log_file = why2_init_logger(WHY2_TEST_DIRECTORY); //INIT LOGGER
    char *key = why2_generate_key(strlen(WHY2_LOGGER_TEST_TEXT) * 2);

    //SET FLAGS
    why2_set_flags
    (
        (why2_input_flags)
        {
            1,
            1,
            0,
            WHY2_v3,
            WHY2_OUTPUT_TEXT
        }
    );

    why2_set_log_flags
    (
        (why2_log_flags)
        {
            key
        }
    );

    why2_write_log(log_file.file, WHY2_LOGGER_TEST_TEXT); //WRITE

    //PRINT
    printf
    (
        "Hi.\n"
        "This is a simple application written using WHY2's logger module.\n\n"

        "Come on, open \"%s\"... I wrote something special there :)\n\n"

        "Thank you so much for supporting this project!\n",

        log_file.filename
    );

    //DEALLOCATION
    why2_deallocate(key);
    why2_deallocate_logger(log_file);

    return 0;
}