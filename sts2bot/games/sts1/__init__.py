"""Slay the Spire (the first game), played by the same bot through CommunicationMod.

    relay.py    CommunicationMod starts it; it passes states and commands to the bot over localhost
    client.py   the bot's client: the same calls as sts2bot.client.Client
    screens.py  the game's state in the words the bot reads
    host.py     starting, finding and stopping the game (for the supervisor)
    text/       what each card and potion says (CommunicationMod sends no card text)
"""
