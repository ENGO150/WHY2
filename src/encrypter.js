module.exports = async (text, key = undefined) => {
	const KEY_LENGTH = 50
        const ENCRYPTION_SEPARATOR_STRING = '.';
        const GENERATE_RANDOM_KEY = async (numberBuffer) => {
            key = [];
            //LOAD KEY
            for (var i = 0; i < KEY_LENGTH; i++) {
                //SET numberBuffer TO RANDOM NUMBER BETWEEN 0 AND 52
                numberBuffer = await crypto.randomInt(0, 52 + 1);

                //GET CHAR FROM numberBuffer
                if (numberBuffer > 26) {    
                    numberBuffer += 70;
                } else {
                    numberBuffer += 64;
                }

                key[i] = numberBuffer;
            }
        }

        //VARIABLES
        var keyRand = KEY_LENGTH;
        var returningText = new String();
        var textBuffer;
        var textKeyChain = [];
        textKeyChain.length = text.length;
        var numberBuffer;

        if (key < KEY_LENGTH) {
            return { exitCode: -2 };
        }
        if (!key) {
            await GENERATE_RANDOM_KEY(numberBuffer);
        }
        //LOAD textKeyChain
        for (var i = 0; i < textKeyChain.length; i++)  {
            numberBuffer = i;

            //CHECK, IF numberBuffer ISN'T GREATER THAN KEY_LENGTH AND CUT UNUSED LENGTH
            while (numberBuffer >= key.length)
            {
                numberBuffer -= key.length;
            }

            if (typeof key === 'string') {
                key = key.split("");
                key = key.map(element => element.charCodeAt(0));
            }

            //FILL textKeyChain
            if ((numberBuffer + 1) % 3 == 0)
            {
                textKeyChain[i] = key[numberBuffer] * key[numberBuffer + 1];
            } else if ((numberBuffer + 1) % 2 == 0)
            {
                textKeyChain[i] = key[numberBuffer] - key[numberBuffer + 1];
            } else
            {
                textKeyChain[i] = key[numberBuffer] + key[numberBuffer + 1];
            }
        }

        //ACTUALLY ENCRYPT TEXT
        for (var i = 0; i < text.length; i++) {
            textKeyChain[i] -= text[i].charCodeAt(0);
        }

        numberBuffer = 0;

        //LOAD returningText
        for (var i = 0; i < textKeyChain.length; i++) {
            textBuffer = Math.floor(Math.log10(Math.abs(textKeyChain[i])));

            textBuffer = textKeyChain[i].toString();

            returningText += textBuffer;

            if (i != textKeyChain.length - 1)
            {
                returningText += ENCRYPTION_SEPARATOR_STRING;
            }

            textBuffer = undefined;
        }
        key = key.map(element => String.fromCharCode(element)).join("");
        return { exitCode: 0, value: returningText, key: key };
}