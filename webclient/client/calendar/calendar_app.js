import {fetch_api} from "../utilities/request";
import {EncString} from "../utilities/encstring";
import {Message, NOTIFICATION} from "../views/message_box/notification";
import {APP_CONFIG} from "../utilities/app_config";
import {Authentication} from "../utilities/authentication/authentication";
import {CalendarUser} from "../utilities/calendar_user";
import {get_week_number} from "../utilities/time_utils";
import {EventPool} from "./event_pool";
import './body/calendar_body'
import {import_ics} from "../utilities/import/ics";
import {EventManager} from "../utilities/event_manager";
import {Selector} from "./selection/selector";

require("./widgets/modal/modal-widgets")
require('./calendar_app.scss');

const TODO_DISABLE_SCROLL = true;

class CalendarApp extends HTMLElement {
    constructor() {
        super();
        /**
         * Start of the display
         * @type {Date}
         * @private
         */
        this._display_date = new Date(Date.now());
        if (this.hasAttribute('display-date'))
            this._display_date = new Date(this.getAttribute('display-date'));
        /**
         * @type {String}
         * @private
         */
        this._title = "";
        if (this.hasAttribute('title'))
            this._title = new Date(this.getAttribute('title'));
        /**
         * The event pool that contains every displayed events
         * @type {EventPool}
         * @private
         */
        this._event_source = null;

        /**
         * @type {EventManager}
         */
        this.events = new EventManager();

        /**
         * @type {CalendarBody}
         * @private
         */
        this._main_body = null;

        /**
         * @type {Selector}
         * @private
         */
        this._selector = new Selector();

        this._current_offset = 0;
        this._touch_start = 0;
        this._touch_start_delta = 0;
        this._holding = false;
        document.addEventListener('touchstart', (event) => {
            this._touch_start = event.targetTouches[0].clientX;
            this._touch_start_delta = this._current_offset;
            this._holding = true;
        })
        document.addEventListener('touchend', (_) => {
            this._holding = false;
        })
        document.addEventListener('touchmove', (event) => {
            this._set_scroll_offset(event.targetTouches[0].clientX - this._touch_start + this._touch_start_delta);
        })
        document.addEventListener("wheel", (event) => {
            this._set_scroll_offset(this._current_offset - event.deltaX * 2);
        })


        class DeltaTime {
            constructor() {
                this._start = Date.now();
            }

            tick() {
                const now = Date.now();
                this._delta = now - this._start;
                this._start = now;
                return this._delta / 1000;
            }

            delta_seconds() {
                return this._delta / 1000;
            }
        }

        this._delta_time = new DeltaTime();

        const lerp = (x, y, a) => {
            if (x > y)
                return Math.max(x * (1 - a) + y * a, y);
            else if (y > x)
                return Math.min(x * (1 - a) + y * a, y);
            return y;
        };

        const scroll_loop = () => {
            requestAnimationFrame(scroll_loop);

            const delta = this._delta_time.tick();
            if (!this._holding) {
                this._current_offset = lerp(this._current_offset, 0, 10 * delta)
                this._current_offset = Math.min(Math.max(this._current_offset, -this._elements.body.clientWidth), this._elements.body.clientWidth)
                this._set_scroll_offset(this._current_offset)

            }
        }

        requestAnimationFrame(scroll_loop)
    }

    _set_scroll_offset(in_new_scroll_offset) {
        if (TODO_DISABLE_SCROLL)
            return; //@TODO : improve scroll ergonomy

        this._current_offset = in_new_scroll_offset;

        if (this._current_offset < -this._elements.body.clientWidth * 0.5) {
            this._current_offset += this._elements.body.clientWidth;
            this._touch_start_delta += this._elements.body.clientWidth;
            const old = this._main_body;
            this._main_body = this._right_body;
            this._right_body = null;

            const date = new Date(this._display_date);
            date.setDate(this._display_date.getDate() + 7);
            this.set_display_date(date)
            this._left_body = old;

            if (!this._main_body)
                return;
            this._main_body.style.transform = 'translate(0)';
            this._main_body.style.position = 'relative';
            this._left_body.style.transform = 'translate(-100%)';
            this._left_body.style.position = 'absolute';
            this._left_body.style.width = '100%';
            this._left_body.style.height = '100%';
        } else if (this._current_offset > this._elements.body.clientWidth * 0.5) {
            this._current_offset -= this._elements.body.clientWidth;
            this._touch_start_delta -= this._elements.body.clientWidth;
            const old = this._main_body;
            this._main_body = this._left_body;
            this._left_body = null;

            const date = new Date(this._display_date);
            date.setDate(this._display_date.getDate() - 7);
            this.set_display_date(date)
            this._right_body = old;

            if (!this._main_body)
                return;
            this._main_body.style.transform = 'translate(0)';
            this._main_body.style.position = 'relative';
            this._right_body.style.transform = 'translate(100%)';
            this._right_body.style.position = 'absolute';
            this._right_body.style.width = '100%';
            this._right_body.style.height = '100%';
        }

        this._elements.body.style.transform = `translate(${this._current_offset}px)`;
        if (this._current_offset > 1 && !this._left_body) {
            const date = new Date(this._display_date);
            date.setDate(this._display_date.getDate() - 7);
            this._left_body = document.createElement('calendar-body');
            this._left_body.set_selector(this._selector);
            this._left_body.set_display_date(date);
            this._left_body.set_event_source(this._event_source);
            this._left_body.style.position = 'absolute';
            this._left_body.style.width = '100%';
            this._left_body.style.height = '100%';
            this._left_body.style.transform = `translate(-100%)`;
            this._elements.body.append(this._left_body)
        } else if (this._current_offset < -1 && !this._right_body) {
            const date = new Date(this._display_date);
            date.setDate(this._display_date.getDate() + 7);
            this._right_body = document.createElement('calendar-body');
            this._right_body.set_selector(this._selector);
            this._right_body.set_display_date(date);
            this._right_body.set_event_source(this._event_source);
            this._right_body.style.position = 'absolute';
            this._right_body.style.width = '100%';
            this._right_body.style.height = '100%';
            this._right_body.style.transform = `translate(100%)`;
            this._elements.body.append(this._right_body)
        }

        if (this._left_body && this._current_offset <= 1) {
            this._left_body.remove();
            this._left_body = null;
        }

        if (this._right_body && this._current_offset >= -1) {
            this._right_body.remove();
            this._right_body = null;
        }

    }

    connectedCallback() {
        const elements = require('./calendar_app.hbs')({
            title: this._make_title()
        }, {
            'import': () => {
                const file_input = document.createElement('input');
                file_input.type = 'file';
                file_input.onchange = async (event) => {
                    await this._event_source.create_events(await import_ics(event.target['files'][0]));
                };
                file_input.click();
            },
            'next_week': () => {
                const date = new Date(this._display_date);
                date.setDate(this._display_date.getDate() + 7);
                this.set_display_date(date);
            },
            'today': () => {
                this.set_display_date(new Date(Date.now()));
            },
            'previous_week': () => {
                const date = new Date(this._display_date);
                date.setDate(this._display_date.getDate() - 7);
                this.set_display_date(date);
            }
        });
        this._elements = elements.elements;
        for (const element of elements)
            this.append(element);

        /** CREAT BODY **/
        this._main_body = document.createElement('calendar-body');
        this._main_body.set_selector(this._selector);
        this._main_body.set_display_date(this._display_date);
        this._main_body.set_event_source(this._event_source);
        this._elements.body.append(this._main_body)
    }

    /**
     * @param try_connect {boolean}
     * @returns {CalendarUser | null}
     */
    async get_connected_user(try_connect = true) {
        if (!try_connect)
            return this._current_calendar_user;

        // Login if required
        if (this._calendar.require_account && !APP_CONFIG.connected_user())
            await Authentication.login();

        if (!APP_CONFIG.connected_user()) {
            // Try to create an unauthenticated user
            this._current_calendar_user = await new Promise((success, failure) => {
                const add_user_form = require('./add_user.hbs')({}, {
                    'show_user_list_options': () => {
                        add_user_form.elements.who_are_you_input.focus();
                        add_user_form.elements.who_are_you_input.value = '';
                    },
                    'value_changed': () => {
                        const value = add_user_form.elements.who_are_you_input.value;
                        if (!value || value === "") {
                            add_user_form.elements.who_I_am.style.display = 'none';
                        } else {
                            add_user_form.elements.who_I_am.style.display = 'flex';
                            if (this._calendar.users.has(value)) {
                                add_user_form.elements.who_I_am.value = `Je suis '${value}'`;
                            } else {
                                add_user_form.elements.who_I_am.value = `Ajouter l'utilisateur '${value}'`;
                            }
                        }
                    },
                    // Try to authenticate
                    'login': async () => {
                        await Authentication.login();
                        this.close_modal();
                        success(null);
                    },
                    'submit': async (event) => {
                        event.preventDefault();
                        const value = add_user_form.elements['who_are_you_input'].value;
                        // The user already exists
                        if (this._calendar.users.has(value)) {
                            this.close_modal();
                            success(this._calendar.users.get(value));
                        } else {
                            await fetch_api('calendar/add_user/', 'POST', {
                                name: EncString.from_client(value),
                                calendar: this._calendar.id.toString(),
                            }).then((res) => {
                                this.close_modal();
                                success(new CalendarUser(res));
                            }).catch(error => {
                                NOTIFICATION.error(new Message(error).title("Impossible de créer l'utilisateur"));
                            });
                        }
                    },
                    'close': () => {
                        this.close_modal();
                        failure();
                    }
                });

                for (const user of this._calendar.users.values()) {
                    const option = document.createElement('option');
                    option.value = user.name.plain();
                    add_user_form.elements.who_are_you_list.append(option);
                }

                this.open_modal(add_user_form);
            });
        }

        if (APP_CONFIG.connected_user()) {
            // We use an authenticated user
            if (this._current_calendar_user && this._current_calendar_user.user_id === APP_CONFIG.connected_user().id)
                return this._current_calendar_user;
            else {
                // Try creating a calendar user from authenticated user
                const res = await fetch_api('calendar/find_or_create_user/', 'POST', {calendar: this._calendar.id.toString()}).catch(error => {
                    NOTIFICATION.error(new Message(error).title("Impossible de créer un utilisateur authentifié"));
                    throw new Error(error);
                });
                this._current_calendar_user = CalendarUser.new(res);
            }
        }

        return this._current_calendar_user;
    }

    /**
     * @returns {Selector}
     */
    selector() {
        return this._selector;
    }

    set_display_date(in_date) {
        if (in_date.getTime() === this._display_date.getTime())
            return;
        this._display_date = in_date;
        if (!this.isConnected)
            return;

        this._elements.title.innerText = this._make_title();

        if (this._main_body)
            this._main_body.set_display_date(this._display_date);
    }

    /**
     * @param event_pool {EventPool}
     */
    set_event_source(event_pool) {
        this._event_source = event_pool;
        if (this._main_body)
            this._main_body.set_event_source(this._event_source);
    }

    _make_title() {
        return `${this._display_date.toLocaleDateString(undefined, {month: 'long'})} ${this._display_date.getFullYear()} - Semaine ${get_week_number(this._display_date)}`
    }

    open_modal(content) {
        this._elements['modal_container'].open(content);
    }

    close_modal() {
        this._elements['modal_container'].close();
    }
}

customElements.define("calendar-app", CalendarApp);