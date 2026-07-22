from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...types import Response, UNSET
from ... import errors

from ...models.api_error_response import ApiErrorResponse
from ...models.key_op_state_response import KeyOpStateResponse
from typing import cast



def _get_kwargs(
    id: int,

) -> dict[str, Any]:
    

    

    

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/accounts/{id}/keyop-state".format(id=quote(str(id), safe=""),),
    }


    return _kwargs



def _parse_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Any | ApiErrorResponse | KeyOpStateResponse | None:
    if response.status_code == 200:
        response_200 = KeyOpStateResponse.from_dict(response.json())



        return response_200

    if response.status_code == 400:
        response_400 = ApiErrorResponse.from_dict(response.json())



        return response_400

    if response.status_code == 404:
        response_404 = cast(Any, None)
        return response_404

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Response[Any | ApiErrorResponse | KeyOpStateResponse]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    id: int,
    *,
    client: AuthenticatedClient | Client,

) -> Response[Any | ApiErrorResponse | KeyOpStateResponse]:
    """ GET /v1/accounts/{id}/keyop-state — public signing state for key operations.

     These digests are already committed validity state and reveal no key or
    portfolio data. A client must fetch them immediately before signing a
    registration or revocation; admission rejects stale values with 409.

    Args:
        id (int):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | ApiErrorResponse | KeyOpStateResponse]
     """


    kwargs = _get_kwargs(
        id=id,

    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)

def sync(
    id: int,
    *,
    client: AuthenticatedClient | Client,

) -> Any | ApiErrorResponse | KeyOpStateResponse | None:
    """ GET /v1/accounts/{id}/keyop-state — public signing state for key operations.

     These digests are already committed validity state and reveal no key or
    portfolio data. A client must fetch them immediately before signing a
    registration or revocation; admission rejects stale values with 409.

    Args:
        id (int):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | ApiErrorResponse | KeyOpStateResponse
     """


    return sync_detailed(
        id=id,
client=client,

    ).parsed

async def asyncio_detailed(
    id: int,
    *,
    client: AuthenticatedClient | Client,

) -> Response[Any | ApiErrorResponse | KeyOpStateResponse]:
    """ GET /v1/accounts/{id}/keyop-state — public signing state for key operations.

     These digests are already committed validity state and reveal no key or
    portfolio data. A client must fetch them immediately before signing a
    registration or revocation; admission rejects stale values with 409.

    Args:
        id (int):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | ApiErrorResponse | KeyOpStateResponse]
     """


    kwargs = _get_kwargs(
        id=id,

    )

    response = await client.get_async_httpx_client().request(
        **kwargs
    )

    return _build_response(client=client, response=response)

async def asyncio(
    id: int,
    *,
    client: AuthenticatedClient | Client,

) -> Any | ApiErrorResponse | KeyOpStateResponse | None:
    """ GET /v1/accounts/{id}/keyop-state — public signing state for key operations.

     These digests are already committed validity state and reveal no key or
    portfolio data. A client must fetch them immediately before signing a
    registration or revocation; admission rejects stale values with 409.

    Args:
        id (int):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | ApiErrorResponse | KeyOpStateResponse
     """


    return (await asyncio_detailed(
        id=id,
client=client,

    )).parsed
